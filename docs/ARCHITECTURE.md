# Architecture

How the pieces fit, and why they are arranged this way.

```
                    ┌──────────────┐
                    │  AniList     │  ids, titles, scores, relations, recs, tags
                    │  GraphQL     │
                    └──────┬───────┘
                           │ creates al:*
                    ┌──────▼───────┐
                    │  Kitsu       │  0..100 rating, cast, voice, staff
                    │  JSON:API    │  joins via `mappings` external ids
                    └──────┬───────┘
                           │ creates ks:*, merges into al:*
                    ┌──────▼───────┐
                    │  Shikimori   │  Russian titles, descriptions, genres
                    │  v1 JSON     │  joins by normalised title
                    └──────┬───────┘
                           │ creates sh:*, merges into al:*/ks:*
                    ┌──────▼───────┐
                    │   SQLite     │  one database, WAL mode
                    │  + FTS5      │  unicode61 tokenizer
                    └──────┬───────┘
                           │
             ┌─────────────┼─────────────┐
             │             │             │
      ┌──────▼─────┐ ┌─────▼──────┐ ┌────▼────────┐
      │  Web UI    │ │ JSON API   │ │ Android app │
      │  (static)  │ │  (Actix)   │ │  (Compose)  │
      └────────────┘ └────────────┘ └─────────────┘
```

## Why one server for both clients

The web UI and the Android app are served from the same origin, so there is
nothing to configure CORS for and one deployment to reason about. The app holds
no catalogue data of its own: it is a client, which means a catalogue fix
reaches every installed app on the next launch with no store update.

## Identity

`uid` is a string, not an integer, and that is the load-bearing decision in the
schema.

Ids from three sources live in three different number spaces. v1 keyed the table
on `anilist_id INTEGER` and encoded "not on AniList" as a negative id from the
same range, so a MAL row numbered 21 and a Shikimori row numbered 21 collided
and `INSERT OR IGNORE` silently dropped one. A prefixed string makes the
collision unrepresentable:

| uid | Source |
|---|---|
| `al:16498` | present on AniList |
| `ks:12` | only Kitsu knows it |
| `sh:34` | only Shikimori knows it |

A Kitsu row that also has an AniList mapping is *not* a new row: it merges into
`al:<id>`, which is how Kitsu ends up contributing a rating and a cast to a
title that AniList created.

## Import order is a dependency chain

```
AniList  → defines the row set
Kitsu    → joins on external ids from `mappings`
Shikimori → joins on the normalised title key
```

Shikimori is last because it has no external ids at all, only names, so it can
only match what the other two already created. It still creates `sh:` rows for
titles nobody else has, which is how a Russian-only entry survives.

Every stage checkpoints into `sync_state`, so an import interrupted by a restart
or a deploy resumes where it stopped.

Genre matching and the FTS index run **per page**, not once at the end. A full
import takes hours at the public rate limits, and doing it at the end meant
search and the genre filter were both empty for the whole of it.

## Search

FTS5 with the `unicode61` tokenizer. The reason is case folding: SQLite's
`LIKE` folds case for ASCII only, so the Russian search in v1 did not work at
all — `АТАКА` never matched `Атака`. `unicode61` folds every script.

If the SQLite build lacks FTS5 the index is skipped, a warning is logged, and
search degrades to `LIKE` scans: weaker for Cyrillic, still functional.

## Ratings on one scale

AniList reports 0..100, Kitsu reports a 0..100 *string*, and the old Jikan path
reported MAL's 0..10. Three scales, one `score` column and a `score_source`.
Kitsu needed no conversion at all, which is one of the reasons it replaced
Jikan.

## Accounts

- Argon2id, one random salt per password.
- Opaque 256-bit session tokens. Only their SHA-256 is stored, so a copy of the
  database does not yield usable credentials, and revoking a session is one
  `DELETE` rather than a token blocklist.
- The UI is fully usable anonymously; an account is only needed for the
  watchlist.

## Request path

```
request
  → CORS (headers; preflights are a real route in the /api scope)
  → Compress
  → Decorate (security headers, request id, access log)
  → handler
      → auth::authenticate  (SHA-256 lookup, blocks a stale token)
      → web::block         (rusqlite is synchronous, so it must leave
                             the async worker threads)
      → query builder      (order clause is a whitelist; filters are bound
                             parameters)
      → pool.get()         (WAL, so reads never block the writer)
  → JSON
```

A few deliberate choices in there:

- **`web::block` everywhere SQLite is touched.** rusqlite is blocking; running
  it on an async worker would stall the runtime.
- **The sort clause is a whitelist.** It is interpolated into SQL, so an
  unvalidated value would be an injection point.
- **Schema is applied once at boot.** v1 re-ran twenty `CREATE INDEX IF NOT
  EXISTS` on every single request.
- **Preflight is a route, not middleware.** A middleware cannot swap a
  response for a different body type without dragging in `EitherBody`, and
  nothing here needs that.

## Android

Kotlin + Compose, three top-level destinations (catalogue, list, settings) behind
a bottom bar, with the detail page and sign-in pushed on top of a tab. Details in
[`android/README.md`](../android/README.md).

Two decisions are worth stating here rather than there.

**The card takes an interface, not a type.** A catalogue row and a watchlist row
carry the same dozen fields, but the watchlist row also carries the viewer's own
score, progress and notes. Modelling that as inheritance or as a conversion would
mean either duplicating the fields or rebuilding a summary per row to render it;
`CardData` lets one `AnimeCard` accept both.

**Two JSON configurations, deliberately opposite.** Decoding sets
`explicitNulls = false` so an installed app survives a server that adds a field.
Encoding sets it to `true`, because `POST /api/favorites` uses PATCH semantics:
absent means *leave alone* and `null` means *clear*. With one shared config,
clearing a score would silently keep the old one.

The dependency container is manual: Hilt would add an annotation processor, build
time and APK size for a graph three objects deep.
