// Page controller for frontend/anime.html. Extracted to its own file so the
// Content-Security-Policy can stay on script-src 'self'.
// Generated from frontend/anime.html -- edit the source HTML, not this file.

(function () {
    'use strict';

    const { I18n, Theme, Api, Session, $, esc, el, add, toast, attachCover, coverUrl, statusLabel, seasonLabel, formatDate } = App;

    const root = $('#root');
    const uid = decodeURIComponent(location.pathname.split('/').filter(Boolean).pop() || '');

    // Built from the shared table so this list cannot fall behind the statuses
    // the API accepts; see the note on `STATUS_LABELS` in app.js.
    const STATUS_OPTIONS = Object.keys(App.statusLabels);

    let data = null;

    // ------------------------------------------------------------- helpers

    function section(titleKey, body, extra) {
        if (!body) return null;
        return el('section', { class: 'section' }, [
            el('h2', { class: 'section__title', text: typeof titleKey === 'string' ? I18n.t(titleKey) : titleKey }),
            body,
            extra || null,
        ].filter(Boolean));
    }

    function tagList(items, cls) {
        if (!items || !items.length) return null;
        return el('div', { class: 'tag-list' }, items.map((it) =>
            el('span', { class: 'tag ' + (cls || ''), text: it })
        ));
    }

    function miniGrid(items) {
        if (!items || !items.length) return null;
        return el('div', { class: 'mini-grid' }, items.map((it) => {
            const art = el('div', { class: 'mini-card__art' });
            attachCover(art, it.cover);
            return el('a', { class: 'mini-card', href: '/anime/' + encodeURIComponent(it.uid) }, [
                art,
                el('div', { class: 'mini-card__body' }, [
                    it.kind ? el('div', { class: 'mini-card__kind', text: it.kind }) : null,
                    el('div', { class: 'mini-card__title', text: it.title }),
                    it.sub ? el('div', { class: 'card__sub', text: it.sub }) : null,
                ].filter(Boolean)),
            ]);
        }));
    }

    function personList(people, isCast) {
        if (!people || !people.length) return null;
        const shown = people.slice(0, isCast ? 40 : 24);
        return el('div', { class: 'person-list' }, shown.map((p) => {
            const img = el('img', { class: 'person__img', alt: '', loading: 'lazy' });
            if (p.image) img.src = coverUrl(p.image);
            const col = el('div', { class: 'person__col' });
            if (isCast && p.role) col.appendChild(el('div', { class: 'person__role', text: p.role }));
            col.appendChild(el('div', { class: 'person__name', text: p.name }));
            if (isCast && p.voice_actor) {
                col.appendChild(el('div', { class: 'person__meta', text: I18n.t('voice_actor') + ': ' + p.voice_actor }));
            }
            if (!isCast && p.positions && p.positions.length) {
                col.appendChild(el('div', { class: 'person__meta', text: p.positions.join(', ') }));
            }
            return el('div', { class: 'person' }, [img, col]);
        }));
    }

    function infoRow(labelKey, value) {
        if (value === null || value === undefined || value === '' ) return null;
        return el('tr', {}, [el('th', { text: I18n.t(labelKey) }), el('td', { html: value })]);
    }

    // ---------------------------------------------------------------- paint

    function paint() {
        document.title = (data.title_russian || data.title_romaji || data.title_english || I18n.t('catalog'));

        const og = document.querySelector('meta[property="og:title"]');
        if (og) og.setAttribute('content', document.title);

        if (data.banner) {
            const banner = $('#banner');
            banner.style.backgroundImage = 'url("' + data.banner.replace(/"/g, '%22') + '")';
        }

        root.innerHTML = '';

        const cover = el('div', { class: 'detail__cover' });
        attachCover(cover, data.cover_large || data.cover_medium || data.cover_small);

        const headings = el('div', { class: 'detail__headings' });
        headings.appendChild(el('h1', { class: 'detail__title', text: mainTitle() }));

        const alts = el('div', { class: 'detail__alts' });
        const altPairs = [
            ['RU', data.title_russian, 'detail__alt detail__alt--ru'],
            ['EN', data.title_english, 'detail__alt'],
            ['JP', data.title_native, 'detail__alt'],
            ['Romaji', data.title_romaji, 'detail__alt'],
        ];
        // Hide the label of whichever field the title itself came from.
        const primary = mainTitle();
        altPairs.forEach(([label, value, cls]) => {
            if (!value || value === primary) return;
            alts.appendChild(el('div', { class: cls, text: label + ': ' + value }));
        });
        if (alts.children.length) headings.appendChild(alts);

        headings.appendChild(pills());

        const actions = el('div', { class: 'detail__actions' });
        const favBtn = el('button', {
            class: 'btn btn--primary',
            type: 'button',
            text: data.library && data.library.is_favorite ? I18n.t('remove_from_list') : I18n.t('add_to_list'),
        });
        favBtn.addEventListener('click', () => editLibrary(favBtn));
        actions.appendChild(favBtn);

        if (data.trailer) {
            const t = el('a', {
                class: 'btn',
                href: data.trailer.url,
                target: '_blank',
                rel: 'noopener noreferrer',
                text: I18n.t('trailer'),
            });
            actions.appendChild(t);
        }
        headings.appendChild(actions);

        root.appendChild(el('div', { class: 'detail__hero' }, [cover, headings]));

        // --- description ---
        const descBlocks = [];
        if (I18n.lang === 'ru') {
            if (data.description_ru) {
                descBlocks.push(el('div', { class: 'section__desc' }, [
                    el('div', { class: 'desc--source', text: I18n.t('description_ru') }),
                    el('div', { class: 'desc desc--ru', text: data.description_ru }),
                ]));
            }
            if (data.description) {
                descBlocks.push(el('div', { class: 'section__desc' }, [
                    el('div', { class: 'desc--source', text: I18n.t('description_en') }),
                    el('div', { class: 'desc', text: data.description }),
                ]));
            }
        } else if (data.description) {
            descBlocks.push(el('div', { class: 'desc', text: data.description }));
        }
        if (descBlocks.length) {
            add(root, section('description', el('div', {}, descBlocks)));
        }

        // --- taxonomy ---
        const genres = (data.genres || []).map((g) => g.name_ru || g.name);
        add(root, section('genres', tagList(genres, 'tag--genre')));
        add(root, section('studios', tagList((data.studios || []).map((s) => s.name_ru || s.name))));
        add(root, section('producers', tagList((data.producers || []).map((s) => s.name_ru || s.name))));
        add(root, section('licensors', tagList((data.licensors || []).map((s) => s.name_ru || s.name))));
        add(root, section('synonyms', tagList(data.synonyms || [])));

        if (data.tags && data.tags.length) {
            add(root, section('tags', el('div', { class: 'tag-list' }, data.tags.map((t) =>
                el('span', {
                    class: 'tag' + (t.spoiler ? ' tag--spoiler' : ''),
                    text: t.name + (t.rank ? ' · ' + t.rank + '%' : ''),
                })
            ))));
        }

        // --- people ---
        add(root, section('cast', personList(data.characters, true)));
        add(root, section('staff', personList(data.staff, false)));

        // --- trailer ---
        if (data.trailer && data.trailer.thumbnail) {
            const box = el('a', {
                class: 'trailer',
                href: data.trailer.url,
                target: '_blank',
                rel: 'noopener noreferrer',
            }, [
                el('img', { src: data.trailer.thumbnail, alt: '', loading: 'lazy' }),
                el('span', { class: 'trailer__play', html: '&#9654;' }),
            ]);
            add(root, section('trailer', box));
        }

        // --- more like this ---
        const recs = (data.recommendations || []).map((r) => ({
            uid: r.uid,
            title: r.title,
            cover: r.cover,
            kind: r.rating ? '★ ' + r.rating : '',
        }));
        add(root, section('recommendations', miniGrid(recs)));

        const relations = (data.relations || []).map((r) => ({
            uid: r.uid,
            title: r.title,
            cover: r.cover,
            kind: r.relation,
            sub: r.format || '',
        }));
        add(root, section('related', miniGrid(relations)));

        // --- info table ---
        add(root, section('info', infoTable()));

        // --- links ---
        if (data.external_links && data.external_links.length) {
            add(root, section('external_links', el('div', { class: 'tag-list' }, data.external_links.map((l) =>
                el('a', {
                    class: 'tag',
                    href: l.url,
                    target: '_blank',
                    rel: 'noopener noreferrer',
                    // The field is called `type` on the wire: the struct field is
                    // `kind`, and it is renamed on serialisation to match the
                    // source vocabulary. Reading `l.kind` silently dropped it.
                    text: l.site + (l.type ? ' · ' + l.type : ''),
                })
            ))));
        }

        if (data.streaming && data.streaming.length) {
            add(root, section('streaming', el('div', { class: 'tag-list' }, data.streaming.slice(0, 40).map((s) =>
                el('a', {
                    class: 'tag',
                    href: s.url,
                    target: '_blank',
                    rel: 'noopener noreferrer',
                    text: s.site + (s.title ? ' · ' + s.title : ''),
                })
            ))));
        }

        const head = $('#head-title');
        head.textContent = primary;
    }

    function mainTitle() {
        if (I18n.lang === 'ru') {
            return data.title_russian || data.title_romaji || data.title_english || data.title_native || '?';
        }
        return data.title_english || data.title_romaji || data.title_native || '?';
    }

    function pills() {
        const box = el('div', { class: 'pills' });
        const add = (text, cls) => {
            if (!text) return;
            box.appendChild(el('span', { class: 'pill ' + (cls || ''), text: text }));
        };

        add(data.format);
        add(data.status);
        if (data.episodes) add(data.episodes + ' ' + I18n.t('episodes'));
        if (data.duration) add(data.duration + ' ' + I18n.t('per_ep'));
        if (data.score) add('★ ' + data.score, 'pill--score');
        if (data.rating_count) add(data.rating_count.toLocaleString() + ' ' + I18n.t('rating_count'));
        if (data.season) add(seasonLabel(data.season) + (data.season_year ? ' ' + data.season_year : ''));
        add(formatDate(data.start_date) || (data.start_date ? '' : ''));
        add(data.country);
        if (data.is_adult) add('18+', 'pill--adult');
        return box;
    }

    function infoTable() {
        const rows = [];
        const ids = data.ids || {};
        // The key travels with the link. It used to be recovered from the label
        // with `label.toLowerCase().slice(0, 3)`, which happens to work for
        // "Shikimori" and "Kitsu" and gives `ani` / `mya` for AniList and
        // MyAnimeList — so the two numbers a reader wants most were the two
        // that never showed.
        const extLinks = [];
        for (const link of [
            { name: 'AniList', key: 'anilist', base: 'https://anilist.co/anime/' },
            { name: 'MyAnimeList', key: 'mal', base: 'https://myanimelist.net/anime/' },
            { name: 'Shikimori', key: 'shikimori', base: 'https://shikimori.one/animes/' },
            { name: 'Kitsu', key: 'kitsu', base: 'https://kitsu.app/anime/' },
        ]) {
            if (!ids[link.key]) continue;
            extLinks.push({
                name: link.name,
                href: link.base + ids[link.key] + (link.key === 'shikimori' ? '/' : ''),
                id: ids[link.key],
            });
        }

        if (extLinks.length) {
            rows.push(el('tr', {}, [
                el('th', { text: I18n.t('ids') }),
                el('td', {}, extLinks.map((link, i) => el('span', {}, [
                    i ? ' · ' : '',
                    el('a', {
                        href: link.href,
                        target: '_blank',
                        rel: 'noopener noreferrer',
                        text: link.name + ' ' + link.id,
                    }),
                ]))),
            ]));
        }

        rows.push(infoRow('format_l', esc(data.format)));
        rows.push(infoRow('status_l', esc(data.status ? statusLabelForSource(data.status) : '')));
        rows.push(infoRow('episodes', esc(data.episodes)));
        rows.push(infoRow('duration', data.duration ? esc(data.duration + ' ' + I18n.t('per_ep')) : ''));
        rows.push(infoRow('season_l', data.season ? esc(seasonLabel(data.season) + (data.season_year ? ' ' + data.season_year : '')) : ''));
        rows.push(infoRow('aired', esc(formatDate(data.start_date))));
        rows.push(infoRow('ended', esc(formatDate(data.end_date))));
        rows.push(infoRow('country_l', esc(data.country)));
        rows.push(infoRow('score_l', data.score
            ? esc(data.score) + (data.score_source ? ' <span style="color:var(--text-faint)">(' + esc(data.score_source) + ')</span>' : '')
            : ''));
        rows.push(infoRow('rating_count', data.rating_count ? data.rating_count.toLocaleString() : ''));
        rows.push(infoRow('popularity', data.popularity ? data.popularity.toLocaleString() : ''));
        rows.push(infoRow('favourites', data.favourites ? data.favourites.toLocaleString() : ''));
        rows.push(infoRow('trending', data.trending ? data.trending.toLocaleString() : ''));
        rows.push(infoRow('age_rating', esc(data.age_rating)));
        if (data.chapters) rows.push(infoRow('episodes', esc(data.chapters)));
        if (data.volumes) rows.push(infoRow('episodes', esc(data.volumes)));

        return el('table', { class: 'info-table' }, rows.filter(Boolean));
    }

    function statusLabelForSource(value) {
        const map = {
            FINISHED: { ru: 'Завершено', en: 'Finished' },
            RELEASING: { ru: 'Выпускается', en: 'Airing' },
            NOT_YET_RELEASED: { ru: 'Ещё не вышел', en: 'Not yet released' },
            HIATUS: { ru: 'Приостановлено', en: 'On hiatus' },
            CANCELLED: { ru: 'Отменено', en: 'Cancelled' },
            PLANNED: { ru: 'Запланировано', en: 'Planned' },
        };
        const entry = map[value];
        if (!entry) return value;
        return I18n.lang === 'ru' ? entry.ru : entry.en;
    }

    // -------------------------------------------------------------- library

    function editLibrary(button) {
        if (!Session.isAuthed) {
            toast(I18n.t('sign_in_required'));
            location.href = '/';
            return;
        }

        const current = data.library || { status: 'planned', is_favorite: false, score: null, progress: null, notes: null };
        const backdrop = el('div', { class: 'modal-backdrop', role: 'dialog', 'aria-modal': 'true' });
        const modal = el('div', { class: 'modal' });

        modal.appendChild(el('h2', { class: 'modal__title', text: I18n.t('my_list') }));
        modal.appendChild(el('p', { class: 'modal__sub', text: mainTitle() }));

        const statusSel = el('select');
        STATUS_OPTIONS.forEach((s) => {
            statusSel.appendChild(el('option', { value: s, text: statusLabel(s) }));
        });
        statusSel.value = current.status;
        // A status the table has no entry for would leave the select on its
        // first option while the panel claims to show what is stored, and the
        // next save would silently change the status. Show the stored value
        // verbatim instead.
        if (!statusSel.value) {
            statusSel.appendChild(el('option', { value: current.status, text: current.status }));
            statusSel.value = current.status;
        }

        const scoreInput = el('input', {
            type: 'number', min: '1', max: '10', inputmode: 'numeric',
            placeholder: '1–10',
        });
        if (current.score) scoreInput.value = current.score;

        const progressInput = el('input', { type: 'number', min: '0', inputmode: 'numeric', placeholder: '0' });
        if (current.progress) progressInput.value = current.progress;
        if (data.episodes) progressInput.placeholder = '0 / ' + data.episodes;

        const favBox = el('input', { type: 'checkbox' });
        favBox.checked = !!current.is_favorite;

        const notesInput = el('textarea', { rows: '3', placeholder: I18n.t('notes_ph') });
        notesInput.value = current.notes || '';

        const field = (labelKey, control) => el('div', { class: 'field' }, [
            el('label', { class: 'field__label', text: I18n.t(labelKey) }),
            control,
        ]);

        modal.appendChild(field('watch_status', statusSel));
        modal.appendChild(el('div', { class: 'field-grid' }, [
            field('your_score', scoreInput),
            field('your_progress', progressInput),
        ]));
        modal.appendChild(el('label', { class: 'switch-row' }, [
            el('span', { class: 'switch-row__label', text: I18n.t('list_favorites') }),
            favBox,
        ]));
        modal.appendChild(field('notes', notesInput));

        const errBox = el('div', { class: 'modal__error', hidden: true });
        modal.appendChild(errBox);

        const saveBtn = el('button', { class: 'btn btn--primary', type: 'button', text: I18n.t('save') });
        const cancelBtn = el('button', { class: 'btn btn--ghost', type: 'button', text: I18n.t('cancel') });

        function close() {
            backdrop.remove();
            document.removeEventListener('keydown', onKey);
        }
        function onKey(e) { if (e.key === 'Escape') close(); }

        cancelBtn.addEventListener('click', close);
        backdrop.addEventListener('click', (e) => { if (e.target === backdrop) close(); });
        document.addEventListener('keydown', onKey);

        saveBtn.addEventListener('click', async () => {
            saveBtn.disabled = true;
            errBox.hidden = true;
            try {
                const payload = {
                    uid: data.uid,
                    status: statusSel.value,
                    is_favorite: favBox.checked,
                    score: scoreInput.value ? Number(scoreInput.value) : null,
                    progress: progressInput.value ? Number(progressInput.value) : null,
                    notes: notesInput.value.trim() || null,
                };
                data.library = await Api.post('/api/favorites', payload);
                button.textContent = data.library.is_favorite
                    ? I18n.t('remove_from_list')
                    : I18n.t('add_to_list');
                close();
                toast(I18n.t('added'));
            } catch (e) {
                errBox.textContent = e.message || I18n.t('error');
                errBox.hidden = false;
            } finally {
                saveBtn.disabled = false;
            }
        });

        modal.appendChild(el('div', { class: 'modal__actions' }, [cancelBtn, saveBtn]));
        backdrop.appendChild(modal);
        document.body.appendChild(backdrop);
        setTimeout(() => statusSel.focus(), 30);
    }

    // ----------------------------------------------------------------- init

    async function init() {
        Theme.apply();
        I18n.setLang(I18n.lang);
        I18n.applyStatic(document);

        $('#btn-theme').addEventListener('click', () => Theme.toggle());
        $('#btn-account').addEventListener('click', () => { location.href = '/'; });

        document.addEventListener('langchange', () => {
            I18n.applyStatic(document);
            if (data) paint();
        });

        if (!uid) {
            root.innerHTML = '';
            root.appendChild(el('div', { class: 'state state--error', style: 'padding-top:180px' }, [
                el('div', { class: 'state__title', text: I18n.t('not_found') }),
                el('div', { class: 'state__hint', text: I18n.t('not_found_hint') }),
            ]));
            return;
        }

        await Session.refresh();

        try {
            data = await Api.get('/api/anime/' + encodeURIComponent(uid));
            paint();
        } catch (e) {
            root.innerHTML = '';
            const state = el('div', { class: 'state state--error', style: 'padding-top:180px' });
            state.appendChild(el('div', {
                class: 'state__title',
                text: e.status === 404 ? I18n.t('not_found') : I18n.t('error'),
            }));
            state.appendChild(el('div', { class: 'state__hint', text: I18n.t('not_found_hint') }));
            const home = el('a', { class: 'btn btn--primary', href: '/', text: I18n.t('catalog') });
            state.appendChild(home);
            root.appendChild(state);
            if (e.status !== 404) console.error(e);
        }
    }

    init();
})();
