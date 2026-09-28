// Page controller for frontend/index.html. Extracted to its own file so the
// Content-Security-Policy can stay on script-src 'self'.
// Generated from frontend/index.html -- edit the source HTML, not this file.

(function () {
    'use strict';

    const { I18n, Theme, Api, Session, $, esc, el, toast, attachCover, debounce, coverUrl, statusLabel, seasonLabel, formatDate } = App;

    // ---------------------------------------------------------------- state

    const SORTS = [
        ['popularity', 'sort_popularity'],
        ['score', 'sort_score'],
        ['score_asc', 'sort_score_asc'],
        ['year', 'sort_year'],
        ['year_asc', 'sort_year_asc'],
        ['title', 'sort_title'],
        ['title_desc', 'sort_title_desc'],
        ['title_ru', 'sort_title_ru'],
        ['favourites', 'sort_favourites'],
        ['trending', 'sort_trending'],
        ['episodes', 'sort_episodes'],
        ['rating_count', 'sort_rating_count'],
        ['added', 'sort_added'],
    ];

    /** Filter state. Mirrors the URL, which is the source of truth on load. */
    // Every key here is also a parameter name on `GET /api/anime`, and the
    // list endpoint answers 400 to an unknown one. `has_russian` used to be
    // called `has_ru` here, so ticking the "Russian title only" box sent
    // `has_ru=yes` and the whole catalogue page came back as an error.
    const defaults = () => ({
        q: '',
        sort: 'popularity',
        format: '',
        status: '',
        season: '',
        genre: '',
        country: '',
        year_from: '',
        year_to: '',
        score_from: '',
        score_to: '',
        adult: '',
        has_russian: '',
        has_trailer: '',
        in_list: '',
    });

    let state = defaults();
    let facets = { formats: [], statuses: [], seasons: [], countries: [], genres: [], year_min: null, year_max: null };
    let page = 1;
    let total = 0;
    let loading = false;
    let exhausted = false;
    let listMode = false;
    let requestSeq = 0;

    // ----------------------------------------------------------------- url

    function readUrl() {
        const p = new URLSearchParams(location.search);
        const next = defaults();
        Object.keys(next).forEach((k) => {
            const v = p.get(k);
            if (v !== null && v !== '') next[k] = v;
        });
        if (!SORTS.some((s) => s[0] === next.sort)) next.sort = 'popularity';
        state = next;
        listMode = next.in_list !== '';
    }

    function writeUrl(replace) {
        const p = new URLSearchParams();
        Object.keys(state).forEach((k) => {
            const v = state[k];
            const isDefault = (k === 'sort' && v === 'popularity') || (v === '' && k !== 'sort') || (k === 'in_list' && v === '');
            if (v && !isDefault) p.set(k, v);
        });
        const url = location.pathname + (p.toString() ? '?' + p.toString() : '');
        history[replace ? 'replaceState' : 'pushState']({}, '', url);
    }

    // -------------------------------------------------------------- render

    function skeletons(n) {
        const frag = document.createDocumentFragment();
        for (let i = 0; i < n; i++) {
            frag.appendChild(el('div', { class: 'card skeleton-card' }, [
                el('div', { class: 'sk sk--art' }),
                el('div', { class: 'card__body' }, [
                    el('div', { class: 'sk sk--line' }),
                    el('div', { class: 'sk sk--line short' }),
                ]),
            ]));
        }
        return frag;
    }

    function card(item, library) {
        const isFav = !!(library && library.is_favorite);
        const art = el('div', { class: 'card__art' });
        attachCover(art, item.cover);

        if (item.score) {
            art.appendChild(el('span', { class: 'card__score', text: '★ ' + item.score }));
        }

        const favBtn = el('button', {
            class: 'card__fav' + (isFav ? ' is-on' : ''),
            type: 'button',
            'aria-pressed': isFav ? 'true' : 'false',
            'aria-label': I18n.t('add_to_list'),
            html: '&#9733;',
        });
        favBtn.addEventListener('click', (e) => {
            e.preventDefault();
            e.stopPropagation();
            toggleFavorite(item, library, favBtn);
        });
        art.appendChild(favBtn);

        const title = I18n.lang === 'ru'
            ? (item.title_russian || item.title)
            : (item.title_english || item.title);
        const main = title;
        const sub = I18n.lang === 'ru'
            ? [item.title, item.title_english].find((x) => x && x !== main) || ''
            : [item.title, item.title_russian].find((x) => x && x !== main) || '';

        const metaLeft = [item.year, item.format].filter(Boolean).join(' · ');

        return el('a', { class: 'card', href: '/anime/' + encodeURIComponent(item.uid) }, [
            art,
            el('div', { class: 'card__body' }, [
                el('div', { class: 'card__title', text: title }),
                sub ? el('div', { class: 'card__sub', text: sub }) : null,
                el('div', { class: 'card__meta' }, [
                    el('span', { text: metaLeft }),
                    item.episodes ? el('span', { text: item.episodes + ' эп.' }) : null,
                ]),
            ].filter(Boolean)),
        ]);
    }

    function renderState(titleKey, hintKey, isError) {
        const node = $('#end-state');
        node.className = 'state' + (isError ? ' state--error' : '');
        node.innerHTML = '';
        node.appendChild(el('div', { class: 'state__title', text: I18n.t(titleKey) }));
        if (hintKey) node.appendChild(el('div', { class: 'state__hint', text: I18n.t(hintKey) }));
        if (isError) {
            const btn = el('button', { class: 'btn btn--primary', type: 'button', text: I18n.t('retry') });
            btn.addEventListener('click', () => { page = 1; exhausted = false; load(true); });
            node.appendChild(btn);
        }
        node.hidden = false;
    }

    // ----------------------------------------------------------------- api

    async function load(reset) {
        if (loading && !reset) return;
        if (exhausted && !reset) return;

        loading = true;
        const grid = $('#grid');
        grid.setAttribute('aria-busy', 'true');

        if (reset) {
            page = 1;
            exhausted = false;
            grid.innerHTML = '';
            grid.appendChild(skeletons(12));
            $('#end-state').hidden = true;
        }

        const seq = ++requestSeq;
        try {
            const data = listMode
                ? await loadList(1)
                : await Api.get('/api/anime', Object.assign({ page: page, per_page: 48 }, state));

            // A newer request already started; drop this response.
            if (seq !== requestSeq) return;

            if (reset) grid.innerHTML = '';
            total = data.total;

            if (!data.items || data.items.length === 0) {
                exhausted = true;
                if (listMode) {
                    renderState('list_empty', 'list_empty_hint', false);
                } else {
                    renderState('nothing', 'nothing_hint', false);
                }
                grid.setAttribute('aria-busy', 'false');
                return;
            }

            const frag = document.createDocumentFragment();
            data.items.forEach((entry) => {
                if (listMode) frag.appendChild(card(entry, entry.library));
                else frag.appendChild(card(entry, null));
            });
            grid.appendChild(frag);

            if (listMode) {
                page = 2;
                exhausted = data.items.length < 48;
            } else {
                page += 1;
                exhausted = !data.has_more;
            }

            updateCount();
        } catch (e) {
            if (seq !== requestSeq) return;
            if (reset) grid.innerHTML = '';
            renderState('error', 'error_hint', true);
            console.error(e);
        } finally {
            loading = false;
            grid.setAttribute('aria-busy', 'false');
        }
    }

    async function loadList(pageNo) {
        const q = { limit: 48, offset: (pageNo - 1) * 48 };
        if (state.in_list) q.status = state.in_list === 'favorites' ? '' : state.in_list;
        if (state.in_list === 'favorites') q.favorites = '1';
        return Api.get('/api/favorites', q);
    }

    function updateCount() {
        const label = I18n.t('found') + ': ' + total.toLocaleString(I18n.lang === 'ru' ? 'ru-RU' : 'en-US');
        $('#brand-count').textContent = label;
    }

    // ------------------------------------------------------------- filters

    function selectField(labelKey, id, options, value, onChange) {
        const sel = el('select', { id: id });
        const any = el('option', { value: '', text: I18n.t('reset') });
        sel.appendChild(any);
        options.forEach((o) => {
            const opt = el('option', { value: o.value, text: o.label });
            sel.appendChild(opt);
        });
        sel.value = value || '';
        sel.addEventListener('change', () => onChange(sel.value));
        return el('div', { class: 'field' }, [
            el('label', { class: 'field__label', for: id, text: I18n.t(labelKey) }),
            sel,
        ]);
    }

    function numberField(labelKey, id, value, placeholderKey, onChange) {
        const input = el('input', {
            type: 'number', id: id, value: value || '',
            placeholder: I18n.t(placeholderKey), inputmode: 'numeric', min: '0',
        });
        input.addEventListener('change', () => onChange(input.value));
        return input;
    }

    function buildFilters() {
        const body = $('#sheet-body');
        body.innerHTML = '';

        body.appendChild(selectField('sort', 'f-sort', SORTS.map((s) => ({ value: s[0], label: I18n.t(s[1]) })), state.sort,
            (v) => { state.sort = v || 'popularity'; applyFast(); }));

        if (facets.formats.length) {
            body.appendChild(selectField('format', 'f-format', facets.formats.map((f) => ({ value: f, label: f })), state.format,
                (v) => { state.format = v; }));
        }

        if (facets.statuses.length) {
            body.appendChild(selectField('status', 'f-status', facets.statuses.map((s) => ({ value: s, label: s })), state.status,
                (v) => { state.status = v; }));
        }

        if (facets.genres.length) {
            body.appendChild(selectField('genre', 'f-genre', facets.genres.slice(0, 120).map((g) => ({ value: g.slug, label: g.name })), state.genre,
                (v) => { state.genre = v; }));
        }

        if (facets.countries.length) {
            body.appendChild(selectField('country', 'f-country', facets.countries.map((c) => ({ value: c, label: c })), state.country,
                (v) => { state.country = v; }));
        }

        if (facets.seasons.length) {
            body.appendChild(selectField('season', 'f-season', facets.seasons.map((s) => ({ value: s, label: seasonLabel(s) })), state.season,
                (v) => { state.season = v; }));
        }

        body.appendChild(el('div', { class: 'field' }, [
            el('label', { class: 'field__label', for: 'f-year-from', text: I18n.t('year') }),
            el('div', { class: 'field--range' }, [
                numberField('year_from', 'f-year-from', state.year_from, 'year_from', (v) => { state.year_from = v; }),
                el('span', { text: '—' }),
                numberField('year_to', 'f-year-to', state.year_to, 'year_to', (v) => { state.year_to = v; }),
            ]),
        ]));

        body.appendChild(el('div', { class: 'field' }, [
            el('label', { class: 'field__label', for: 'f-score-from', text: I18n.t('score') }),
            el('div', { class: 'field--range' }, [
                numberField('score_from', 'f-score-from', state.score_from, 'score_from', (v) => { state.score_from = v; }),
                el('span', { text: '—' }),
                numberField('score_to', 'f-score-to', state.score_to, 'score_to', (v) => { state.score_to = v; }),
            ]),
        ]));

        body.appendChild(selectField('adult', 'f-adult', [
            { value: '', label: I18n.t('adult_any') },
            { value: 'no', label: I18n.t('adult_safe') },
            { value: 'only', label: I18n.t('adult_only') },
        ], state.adult, (v) => { state.adult = v; }));

        body.appendChild(selectField('has_russian', 'f-has-ru', [
            { value: '', label: I18n.t('has_ru_any') },
            { value: 'yes', label: I18n.t('has_ru_yes') },
        ], state.has_russian, (v) => { state.has_russian = v; }));

        const trailer = el('label', { class: 'switch-row' }, [
            el('span', {}, [
                el('span', { class: 'switch-row__label', text: I18n.t('has_trailer') }),
            ]),
            (() => {
                const cb = el('input', { type: 'checkbox' });
                cb.checked = state.has_trailer === 'yes';
                cb.addEventListener('change', () => { state.has_trailer = cb.checked ? 'yes' : ''; });
                return cb;
            })(),
        ]);
        body.appendChild(el('div', { class: 'field' }, [trailer]));
    }

    function activeFilterChips() {
        const bar = $('#filterbar');
        bar.innerHTML = '';

        const openBtn = el('button', { class: 'chip chip--filter', type: 'button' });
        const count = countActive();
        openBtn.appendChild(el('span', { html: '&#9881;' }));
        openBtn.appendChild(el('span', { text: I18n.t('filters') }));
        if (count) openBtn.appendChild(el('span', { class: 'chip__badge', text: String(count) }));
        openBtn.addEventListener('click', openSheet);
        bar.appendChild(openBtn);

        if (listMode) {
            const chip = el('button', { class: 'chip is-active', type: 'button', text: I18n.t('my_list') });
            chip.addEventListener('click', () => { state.in_list = ''; listMode = false; apply(); });
            bar.appendChild(chip);
        }

        SORTS.forEach(([value, key]) => {
            if (state.sort === value || value === 'popularity') return;
            const chip = el('button', { class: 'chip is-active', type: 'button', text: I18n.t(key) });
            chip.addEventListener('click', () => { state.sort = value; apply(); });
            bar.appendChild(chip);
        });

        [
            ['format', state.format],
            ['status', state.status],
            ['genre', state.genre],
            ['country', state.country],
            ['season', state.season ? seasonLabel(state.season) : ''],
            ['adult', state.adult === 'no' ? I18n.t('adult_safe') : state.adult === 'only' ? I18n.t('adult_only') : ''],
            ['has_russian', state.has_russian === 'yes' ? I18n.t('has_ru_yes') : ''],
            ['has_trailer', state.has_trailer === 'yes' ? I18n.t('has_trailer') : ''],
        ].forEach(([key, label]) => {
            if (!label) return;
            const chip = el('button', { class: 'chip is-active', type: 'button', text: label + '  \u00d7' });
            chip.addEventListener('click', () => { state[key] = ''; apply(); });
            bar.appendChild(chip);
        });

        if (state.year_from || state.year_to) {
            const label = (state.year_from || '…') + '–' + (state.year_to || '…');
            const chip = el('button', { class: 'chip is-active', type: 'button', text: label + '  \u00d7' });
            chip.addEventListener('click', () => { state.year_from = ''; state.year_to = ''; apply(); });
            bar.appendChild(chip);
        }

        if (state.score_from || state.score_to) {
            const label = (state.score_from || '…') + '–' + (state.score_to || '…');
            const chip = el('button', { class: 'chip is-active', type: 'button', text: I18n.t('score') + ' ' + label + '  \u00d7' });
            chip.addEventListener('click', () => { state.score_from = ''; state.score_to = ''; apply(); });
            bar.appendChild(chip);
        }
    }

    function countActive() {
        let n = 0;
        ['format', 'status', 'season', 'genre', 'country', 'adult', 'has_russian', 'has_trailer'].forEach((k) => {
            if (state[k]) n++;
        });
        if (state.year_from || state.year_to) n++;
        if (state.score_from || state.score_to) n++;
        return n;
    }

    // -------------------------------------------------------------- sheet

    function openSheet() {
        $('#sheet').hidden = false;
        $('#sheet-backdrop').hidden = false;
        document.body.classList.add('body--sheet-open');
    }

    function closeSheet() {
        $('#sheet').hidden = true;
        $('#sheet-backdrop').hidden = true;
        document.body.classList.remove('body--sheet-open');
    }

    // ----------------------------------------------------------- favourites

    async function toggleFavorite(item, library, button) {
        if (!Session.isAuthed) {
            toast(I18n.t('sign_in_required'));
            Account.open('login');
            return;
        }
        const wasFav = !!(library && library.is_favorite);
        try {
            if (wasFav) {
                await Api.del('/api/favorites/' + encodeURIComponent(item.uid));
                toast(I18n.t('removed'));
            } else {
                await Api.post('/api/favorites', { uid: item.uid, is_favorite: true, status: 'planned' });
                toast(I18n.t('added'));
            }
            if (button) button.classList.toggle('is-on', !wasFav);
            if (listMode) load(true);
        } catch (e) {
            toast(e.message || I18n.t('error'), true);
        }
    }

    // ------------------------------------------------------------- account

    const Account = {
        open(mode) {
            const isLogin = mode !== 'signup';
            const backdrop = el('div', { class: 'modal-backdrop', role: 'dialog', 'aria-modal': 'true' });
            const modal = el('div', { class: 'modal' });

            const tabs = el('div', { class: 'tabs' });
            const tabLogin = el('button', { class: 'tab' + (isLogin ? ' is-active' : ''), type: 'button', text: I18n.t('sign_in') });
            const tabSignup = el('button', { class: 'tab' + (!isLogin ? ' is-active' : ''), type: 'button', text: I18n.t('sign_up') });
            tabs.append(tabLogin, tabSignup);
            modal.appendChild(tabs);

            const errBox = el('div', { class: 'modal__error', hidden: true });
            const loginField = el('input', { type: 'text', id: 'acc-login', autocomplete: 'username', placeholder: I18n.t('login_or_email') });
            const userField = el('input', { type: 'text', id: 'acc-user', autocomplete: 'username', placeholder: I18n.t('username') });
            const emailField = el('input', { type: 'email', id: 'acc-email', autocomplete: 'email', placeholder: I18n.t('email') });
            const passField = el('input', { type: 'password', id: 'acc-pass', autocomplete: 'current-password', placeholder: I18n.t('password') });

            const wrapField = (labelKey, input) => el('div', { class: 'field' }, [
                el('label', { class: 'field__label', for: input.id, text: I18n.t(labelKey) }),
                input,
            ]);

            const fields = el('div');
            const submit = el('button', { class: 'btn btn--primary', type: 'submit', text: I18n.t('sign_in') });

            const form = el('form', { novalidate: true }, [
                errBox,
                fields,
                el('div', { class: 'modal__actions' }, [
                    el('button', { class: 'btn btn--ghost', type: 'button', text: I18n.t('cancel'), onclick: close }),
                    submit,
                ]),
            ]);

            function paint() {
                fields.innerHTML = '';
                errBox.hidden = true;
                if (isLogin) {
                    fields.append(wrapField('login_or_email', loginField), wrapField('password', passField));
                    passField.setAttribute('autocomplete', 'current-password');
                    submit.textContent = I18n.t('sign_in');
                } else {
                    fields.append(
                        wrapField('username', userField),
                        wrapField('email', emailField),
                        wrapField('password', passField)
                    );
                    passField.setAttribute('autocomplete', 'new-password');
                    submit.textContent = I18n.t('sign_up');
                }
                tabLogin.classList.toggle('is-active', isLogin);
                tabSignup.classList.toggle('is-active', !isLogin);
            }

            function close() {
                backdrop.remove();
                document.removeEventListener('keydown', onKey);
            }

            function onKey(e) {
                if (e.key === 'Escape') close();
            }

            tabLogin.addEventListener('click', () => { isLogin = true; paint(); });
            tabSignup.addEventListener('click', () => { isLogin = false; paint(); });
            backdrop.addEventListener('click', (e) => { if (e.target === backdrop) close(); });
            document.addEventListener('keydown', onKey);
            paint();

            form.addEventListener('submit', async (e) => {
                e.preventDefault();
                submit.disabled = true;
                errBox.hidden = true;
                try {
                    let res;
                    if (isLogin) {
                        res = await Api.post('/api/auth/login', { login: loginField.value.trim(), password: passField.value });
                    } else {
                        res = await Api.post('/api/auth/register', {
                            username: userField.value.trim(),
                            email: emailField.value.trim() || undefined,
                            password: passField.value,
                        });
                    }
                    Session.set(res.token, res.user);
                    close();
                    paintAccount();
                    toast(I18n.t('account') + ': ' + res.user.username);
                    if (listMode) load(true);
                } catch (err) {
                    errBox.textContent = err.message || I18n.t('error');
                    errBox.hidden = false;
                } finally {
                    submit.disabled = false;
                }
            });

            modal.appendChild(el('h2', { class: 'modal__title', text: I18n.t('account') }));
            modal.appendChild(form);
            backdrop.appendChild(modal);
            document.body.appendChild(backdrop);
            setTimeout(() => (isLogin ? loginField : userField).focus(), 30);
        },
    };

    function paintAccount() {
        const btn = $('#btn-account');
        if (Session.user) {
            btn.textContent = (Session.user.username || '?').trim().charAt(0).toUpperCase();
            btn.title = Session.user.username;
            btn.removeAttribute('href');
            btn.onclick = (e) => {
                e.preventDefault();
                openAccountSheet();
            };
        } else {
            btn.textContent = '?';
            btn.title = I18n.t('sign_in');
            btn.removeAttribute('href');
            btn.onclick = (e) => { e.preventDefault(); Account.open('login'); };
        }
    }

    /** Signed-in menu: the list plus a deliberate sign-out button. */
    function openAccountSheet() {
        const backdrop = el('div', { class: 'modal-backdrop', role: 'dialog', 'aria-modal': 'true' });
        const modal = el('div', { class: 'modal', style: 'max-width:340px' });
        modal.appendChild(el('h2', { class: 'modal__title', text: Session.user.username }));
        if (Session.user.email) {
            modal.appendChild(el('p', { class: 'modal__sub', text: Session.user.email }));
        }

        const listBtn = el('button', { class: 'btn', type: 'button', text: I18n.t('my_list') });
        listBtn.addEventListener('click', () => {
            close();
            state.in_list = state.in_list ? '' : 'favorites';
            listMode = !!state.in_list;
            if (listMode) state.sort = 'popularity';
            apply();
        });

        const outBtn = el('button', { class: 'btn btn--ghost', type: 'button', text: I18n.t('sign_out') });
        outBtn.addEventListener('click', async () => {
            outBtn.disabled = true;
            try {
                await Api.post('/api/auth/logout');
            } catch (e) { /* the local session is dropped either way */ }
            Session.set(null, null);
            close();
            paintAccount();
            toast(I18n.t('sign_out'));
        });

        modal.appendChild(el('div', { class: 'modal__actions' }, [listBtn, outBtn]));
        backdrop.appendChild(modal);

        function close() {
            backdrop.remove();
            document.removeEventListener('keydown', onKey);
        }
        function onKey(e) {
            if (e.key === 'Escape') close();
        }

        backdrop.addEventListener('click', (e) => { if (e.target === backdrop) close(); });
        document.addEventListener('keydown', onKey);
        document.body.appendChild(backdrop);
    }

    // -------------------------------------------------------------- search

    const suggestBox = () => $('#suggest');
    const searchInput = () => $('#search-input');

    let suggestItems = [];
    let suggestIndex = -1;

    async function fetchSuggest(term) {
        const box = suggestBox();
        if (!term || term.length < 2) {
            box.hidden = true;
            return;
        }
        try {
            const items = await Api.get('/api/search/suggest', { q: term, limit: 8 });
            suggestItems = items || [];
            suggestIndex = -1;
            box.innerHTML = '';
            if (!suggestItems.length) {
                box.hidden = true;
                return;
            }
            suggestItems.forEach((item, i) => {
                const row = el('a', { class: 'suggest__item', href: '/anime/' + encodeURIComponent(item.uid), role: 'option' });
                if (item.cover) {
                    const img = el('img', { alt: '', loading: 'lazy' });
                    img.src = coverUrl(item.cover);
                    row.appendChild(img);
                }
                const titles = el('div', { class: 'suggest__titles' });
                titles.appendChild(el('div', { class: 'suggest__title', text: item.title }));
                const sub = [item.title_romaji, item.title_english, item.title_native]
                    .filter((x) => x && x !== item.title);
                if (sub.length) titles.appendChild(el('div', { class: 'suggest__sub', text: sub.join(' · ') }));
                row.appendChild(titles);
                row.addEventListener('mouseenter', () => setSuggestActive(i));
                box.appendChild(row);
            });
            box.hidden = false;
            searchInput().setAttribute('aria-expanded', 'true');
        } catch (e) {
            box.hidden = true;
        }
    }

    function setSuggestActive(index) {
        const rows = suggestBox().querySelectorAll('.suggest__item');
        rows.forEach((r, i) => r.classList.toggle('is-active', i === index));
        suggestIndex = index;
    }

    function commitSearch(term) {
        state.q = term.trim();
        suggestBox().hidden = true;
        searchInput().setAttribute('aria-expanded', 'false');
        apply();
    }

    // ---------------------------------------------------------------- apply

    /** Sorting and other instant filters do not need the sheet. */
    function applyFast() {
        activeFilterChips();
        apply();
    }

    function apply() {
        writeUrl(true);
        closeSheet();
        activeFilterChips();
        load(true);
    }

    // ----------------------------------------------------------------- init

    async function loadFacets() {
        try {
            const [f, g] = await Promise.all([
                Api.get('/api/filters'),
                Api.get('/api/genres', { category: 'genre', min_count: 3 }),
            ]);
            facets = Object.assign(facets, f, { genres: (g && g.genres) || [] });
            buildFilters();
        } catch (e) {
            console.warn('filters unavailable', e);
        }
    }

    function bind() {
        $('#btn-theme').addEventListener('click', () => Theme.toggle());

        $('#sheet-close').addEventListener('click', closeSheet);
        $('#sheet-backdrop').addEventListener('click', closeSheet);
        $('#sheet-apply').addEventListener('click', apply);
        $('#sheet-reset').addEventListener('click', () => {
            const sort = state.sort;
            state = defaults();
            state.sort = sort;
            buildFilters();
            apply();
        });

        $('#btn-list').addEventListener('click', () => {
            if (!Session.isAuthed) {
                toast(I18n.t('sign_in_required'));
                Account.open('login');
                return;
            }
            state.in_list = state.in_list ? '' : 'favorites';
            listMode = !!state.in_list;
            if (listMode) {
                // Sorting a personal list is meaningless, so drop it.
                state.sort = 'popularity';
            }
            apply();
        });

        const input = searchInput();
        input.addEventListener('input', debounce(() => {
            $('#search-clear').hidden = !input.value;
            fetchSuggest(input.value);
        }, 200));

        input.addEventListener('keydown', (e) => {
            const rows = suggestBox().querySelectorAll('.suggest__item');
            if (e.key === 'ArrowDown' && rows.length) {
                e.preventDefault();
                setSuggestActive((suggestIndex + 1) % rows.length);
            } else if (e.key === 'ArrowUp' && rows.length) {
                e.preventDefault();
                setSuggestActive((suggestIndex - 1 + rows.length) % rows.length);
            } else if (e.key === 'Enter') {
                if (suggestIndex >= 0 && rows[suggestIndex]) {
                    e.preventDefault();
                    location.href = rows[suggestIndex].getAttribute('href');
                } else {
                    commitSearch(input.value);
                }
            } else if (e.key === 'Escape') {
                suggestBox().hidden = true;
            }
        });

        $('#search-clear').addEventListener('click', () => {
            input.value = '';
            state.q = '';
            $('#search-clear').hidden = true;
            suggestBox().hidden = true;
            apply();
        });

        document.addEventListener('click', (e) => {
            if (!e.target.closest('.search')) suggestBox().hidden = true;
        });

        document.addEventListener('langchange', () => {
            I18n.applyStatic(document);
            buildFilters();
            activeFilterChips();
            paintAccount();
            updateCount();
            load(true);
        });

        window.addEventListener('popstate', () => {
            readUrl();
            buildFilters();
            activeFilterChips();
            load(true);
        });

        // Infinite scroll. The sentinel is a 1px element after the grid, so
        // reaching it means the user is near the end of the list.
        const io = new IntersectionObserver((entries) => {
            if (entries.some((en) => en.isIntersecting) && !loading && !exhausted) load(false);
        }, { rootMargin: '600px 0px' });
        io.observe($('#sentinel'));

        // A tap near the top on mobile should reveal the search, which is
        // hidden behind the brand on narrow screens.
        let lastY = window.scrollY;
        window.addEventListener('scroll', () => {
            const y = window.scrollY;
            if (y < 120 && y < lastY) window.scrollTo({ top: 0, behavior: 'smooth' });
            lastY = y;
        }, { passive: true });
    }

    async function init() {
        Theme.apply();
        I18n.setLang(I18n.lang);
        I18n.applyStatic(document);
        document.title = I18n.t('catalog');

        readUrl();
        bind();
        activeFilterChips();
        searchInput().value = state.q;
        $('#search-clear').hidden = !state.q;

        await Session.refresh();
        paintAccount();

        loadFacets();
        load(true);
    }

    init();
})();
