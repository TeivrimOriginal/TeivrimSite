//! Periodic catalogue refresh.
//!
//! Before this existed the loaders ran once, on boot, and then the catalogue
//! was frozen for the life of the process: a title that started airing after
//! the last pass never appeared, and a rating that moved never moved here.
//! A deployment only got a refresh by being restarted or by somebody with the
//! admin token pressing the button.
//!
//! The loop is deliberately dumb — sleep, rewind the checkpoints, run — and
//! every decision it makes is a function next to it, so the interesting part
//! (is a pass allowed to start, and is a schedule even wanted) is testable
//! without a clock and without a network.

use super::{human_secs, run_claimed, try_begin_run, Ctx};
use crate::db;
use crate::error::{log_error, log_info};
use std::time::Duration;

/// What one tick of the timer does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tick {
    /// A pass is already in flight. The tick is dropped and the next one comes
    /// round in a full interval.
    ///
    /// Skipping rather than queueing is the point: a full pass takes longer
    /// than most intervals, so a queue would grow without bound and every entry
    /// in it would be stale by the time it ran.
    Busy,
    /// Start a pass.
    Run,
}

/// The decision the loop makes on every tick, as a pure function.
pub fn tick(a_pass_is_running: bool) -> Tick {
    if a_pass_is_running {
        Tick::Busy
    } else {
        Tick::Run
    }
}

/// Whether an interval asks for a schedule at all.
///
/// `0` is the off switch, and it has to be checked before the loop is spawned:
/// `sleep(ZERO)` returns immediately, so a disabled schedule that still ran
/// the loop would spin a core and hammer the sources as fast as they answer.
pub fn enabled(interval: Duration) -> bool {
    !interval.is_zero()
}

/// Starts the refresh loop. Returns false when the schedule is disabled, so
/// the caller can say so once at boot instead of silently doing nothing.
pub fn spawn(ctx: Ctx, interval: Duration) -> bool {
    if !enabled(interval) {
        log_info("[sync] автообновление отключено (SYNC_INTERVAL_SECS=0)");
        return false;
    }
    log_info(&format!(
        "[sync] автообновление каталога каждые {}",
        human_secs(interval.as_secs())
    ));

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            match tick(super::is_running()) {
                Tick::Busy => {
                    log_info("[sync] предыдущий проход ещё идёт, этот тик пропущен");
                    continue;
                }
                Tick::Run => {}
            }
            if let Some(_guard) = try_begin_run() {
                refresh(&ctx).await;
            }
        }
    });
    true
}

/// One refresh: forget every finished task, then walk the sources again.
async fn refresh(ctx: &Ctx) {
    if let Err(e) = rewind(ctx) {
        // Without the rewind every sort is skipped as "already synchronised"
        // and the pass is a no-op that still costs a few thousand requests, so
        // it is better to skip it entirely than to run it empty.
        log_error(&format!("[sync] сброс чекпойнтов не удался, проход пропущен: {}", e));
        return;
    }
    log_info("=== плановое обновление каталога ===");
    run_claimed(ctx.clone()).await;
    report_cache(ctx);
}

/// One line per source saying how much the response cache did.
///
/// The claim that catalogue pages are never reused and reference documents
/// always are lives in `upstream::Freshness`. A number in the log is the only
/// version of that claim that stays true.
fn report_cache(ctx: &Ctx) {
    for (name, up) in [
        ("anilist", &ctx.sources.anilist),
        ("kitsu", &ctx.sources.kitsu),
        ("shikimori", &ctx.sources.shikimori),
    ] {
        let s = up.cache_stats();
        if s.hits == 0 && s.refused == 0 {
            continue;
        }
        log_info(&format!(
            "[{}] кэш ответов: {} записей ({} байт), попаданий {}, промахов {}, не влезло {}",
            name, s.entries, s.bytes, s.hits, s.misses, s.refused
        ));
    }
}

/// Clears the `finished` flag on every task. Split out of [`refresh`] because
/// it is the one step that can fail on its own and has to be tested apart from
/// the loaders.
fn rewind(ctx: &Ctx) -> Result<usize, String> {
    let conn = ctx.db.conn().map_err(|e| e.to_string())?;
    db::rewind(&conn).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::testing::conn;
    use crate::db::{get_checkpoint, mark_error, save_checkpoint};
    use std::time::Instant;

    // ------------------------------------------------------------ the tick

    #[test]
    fn a_free_loader_runs_on_the_tick() {
        assert_eq!(tick(false), Tick::Run);
    }

    #[test]
    fn a_busy_loader_misses_the_tick() {
        // A pass takes longer than a typical interval, so queueing would build
        // a backlog of work that is stale before it starts.
        assert_eq!(tick(true), Tick::Busy);
    }

    #[test]
    fn a_zero_interval_is_the_off_switch() {
        assert!(!enabled(Duration::ZERO));
        assert!(enabled(Duration::from_secs(1)));
    }

    // ---------------------------------------------------------- the rewind

    /// A finished task is a "never look at this again" flag, so a refresh that
    /// leaves it set is a refresh that imports nothing.
    #[test]
    fn a_rewind_reopens_a_finished_task_from_the_first_page() {
        let c = conn();
        save_checkpoint(&c, "anilist", "sort:ID", 900, 45_000, true).unwrap();
        assert!(get_checkpoint(&c, "anilist", "sort:ID").finished);

        assert_eq!(rewind_all(&c), 1);
        let cp = get_checkpoint(&c, "anilist", "sort:ID");
        assert!(!cp.finished, "задача осталась помеченной как завершённая");
        // `last_page` matters as much: an unfinished task that resumes at 901
        // would ask for one page and finish.
        assert_eq!(cp.last_page, 0, "счётчик страниц не сброшен");
        assert_eq!(cp.total_saved, 0);
    }

    #[test]
    fn a_rewind_clears_a_stale_error() {
        // A task that failed halfway through the previous pass should not
        // still be red on the progress page after the next one walked past it.
        let c = conn();
        save_checkpoint(&c, "shikimori", "order:rating", 4, 200, true).unwrap();
        mark_error(&c, "shikimori", "order:rating", "HTTP 503");

        rewind_all(&c);
        let last: Option<String> = c
            .query_row(
                "SELECT last_error FROM sync_state WHERE source = 'shikimori' AND task = 'order:rating'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(last.is_none(), "ошибка осталась: {:?}", last);
    }

    #[test]
    fn a_rewind_does_not_clear_the_completion_time_of_a_run() {
        // The `run` rows are how the progress page shows when a source was
        // last entered; zeroing their timestamp would make the dashboard say a
        // source has never run.
        let c = conn();
        crate::db::mark_started(&c, "kitsu").unwrap();
        rewind_all(&c);
        let started: Option<i64> = c
            .query_row(
                "SELECT started_at FROM sync_state WHERE source = 'kitsu' AND task = 'run'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(started.is_some(), "время старта пропало");
    }

    #[test]
    fn a_rewind_of_every_source_at_once_reopens_them_all() {
        // One statement, so a refresh cannot forget one source and re-import
        // the other two.
        let c = conn();
        for (source, task) in [
            ("anilist", "sort:TRENDING_DESC"),
            ("kitsu", "sort:-popularityRank"),
            ("kitsu", "enrich"),
            ("shikimori", "order:popularity"),
        ] {
            save_checkpoint(&c, source, task, 100, 5_000, true).unwrap();
        }
        assert_eq!(rewind_all(&c), 4);
        let left: i64 = c
            .query_row("SELECT COUNT(*) FROM sync_state WHERE finished = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn a_rewind_on_an_empty_database_is_not_an_error() {
        let c = conn();
        assert_eq!(rewind_all(&c), 0);
    }

    #[test]
    fn a_rewind_is_cheap_enough_to_run_every_pass() {
        // It runs on the hot path of every refresh, so it must not rewrite the
        // world: one UPDATE, no table scan of the catalogue.
        let c = conn();
        for i in 0..50 {
            save_checkpoint(&c, "anilist", &format!("sort:S{}", i), 10, 100, true).unwrap();
        }
        let started = Instant::now();
        assert_eq!(rewind_all(&c), 50);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "перемотка заняла {:?}",
            started.elapsed()
        );
    }

    fn rewind_all(c: &rusqlite::Connection) -> usize {
        db::rewind(c).expect("rewind")
    }
}
