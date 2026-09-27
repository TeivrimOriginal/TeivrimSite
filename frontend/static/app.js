/* ==========================================================================
   Shared frontend runtime: i18n, API client, session, DOM helpers, toasts.
   Loaded as a classic script (not a module) so it is available synchronously
   to both pages and works from file:// during local debugging.
   ========================================================================== */

(function (global) {
    'use strict';

    // ---------------------------------------------------------------- i18n

    const STRINGS = {
        ru: {
            catalog: 'Каталог аниме',
            search_ph: 'Поиск по названию…',
            found: 'Найдено',
            loading: 'Загрузка…',
            nothing: 'Ничего не найдено',
            nothing_hint: 'Попробуйте изменить фильтры или поисковый запрос',
            error: 'Не удалось загрузить',
            error_hint: 'Проверьте соединение и попробуйте ещё раз',
            retry: 'Повторить',
            filters: 'Фильтры',
            apply: 'Показать',
            reset: 'Сбросить',
            clear_all: 'Сбросить всё',
            sort: 'Сортировка',
            sort_popularity: 'По популярности',
            sort_score: 'По оценке',
            sort_score_asc: 'По оценке (сначала низкие)',
            sort_year: 'Сначала новые',
            sort_year_asc: 'Сначала старые',
            sort_title: 'По названию',
            sort_title_desc: 'По названию (обратно)',
            sort_title_ru: 'По русскому названию',
            sort_favourites: 'По избранному',
            sort_trending: 'По тренду',
            sort_episodes: 'По количеству эпизодов',
            sort_rating_count: 'По числу оценок',
            sort_added: 'Сначала добавленные',
            format: 'Формат',
            status: 'Статус',
            season: 'Сезон',
            genre: 'Жанр',
            country: 'Страна',
            year: 'Год',
            year_from: 'Год с',
            year_to: 'Год по',
            score: 'Оценка',
            score_from: 'от',
            score_to: 'до',
            adult: 'Возраст',
            adult_any: 'Любой',
            adult_safe: 'Без 18+',
            adult_only: 'Только 18+',
            has_ru: 'Русское название',
            has_ru_any: 'Любой',
            has_ru_yes: 'Только с русским',
            has_trailer: 'Только с трейлером',
            theme: 'Тема',
            theme_dark: 'Тёмная',
            theme_light: 'Светлая',
            account: 'Аккаунт',
            sign_in: 'Войти',
            sign_up: 'Регистрация',
            sign_out: 'Выйти',
            username: 'Имя пользователя',
            email: 'E-mail (необязательно)',
            password: 'Пароль',
            login_or_email: 'Логин или e-mail',
            no_account: 'Нет аккаунта?',
            have_account: 'Уже есть аккаунт?',
            my_list: 'Мой список',
            list_watching: 'Смотрю',
            list_planned: 'Запланировано',
            list_completed: 'Просмотрено',
            list_dropped: 'Брошено',
            list_favorites: 'Избранное',
            list_all: 'Всё',
            list_empty: 'Список пуст',
            list_empty_hint: 'Нажмите на звездочку у любого аниме, чтобы добавить его сюда',
            add_to_list: 'В список',
            remove_from_list: 'Убрать из списка',
            added: 'Добавлено',
            removed: 'Убрано',
            sign_in_required: 'Войдите, чтобы вести свой список',
            back: 'Назад',
            to_top: 'Наверх',
            not_found: 'Аниме не найдено',
            not_found_hint: 'Возможно, ссылка устарела или запись ещё не загружена',
            description: 'Описание',
            description_ru: 'Описание (Shikimori)',
            description_en: 'Описание (AniList)',
            genres: 'Жанры',
            tags: 'Теги',
            studios: 'Студии',
            producers: 'Продюсеры',
            licensors: 'Лицензиары',
            cast: 'Персонажи и озвучка',
            staff: 'Создатели',
            related: 'Связанные работы',
            recommendations: 'Похожее',
            external_links: 'Внешние ссылки',
            streaming: 'Смотреть онлайн',
            trailer: 'Трейлер',
            info: 'Информация',
            ids: 'ID',
            format_l: 'Формат',
            status_l: 'Статус',
            episodes: 'Эпизоды',
            duration: 'Длительность',
            per_ep: 'мин/эп',
            season_l: 'Сезон',
            aired: 'Дата выхода',
            ended: 'Дата окончания',
            country_l: 'Страна',
            score_l: 'Оценка',
            score_source: 'Источник оценки',
            rating_count: 'Оценок',
            popularity: 'Популярность',
            favourites: 'В избранном',
            trending: 'Тренд',
            synonyms: 'Синонимы',
            age_rating: 'Возрастной рейтинг',
            voice_actor: 'Озвучка',
            your_score: 'Ваша оценка',
            your_progress: 'Прогресс',
            episodes_watched: 'эпизодов',
            save: 'Сохранить',
            cancel: 'Отмена',
            delete: 'Удалить',
            notes: 'Заметки',
            notes_ph: 'Личные заметки…',
            watch_status: 'Статус',
            season_winter: 'Зима',
            season_spring: 'Весна',
            season_summer: 'Лето',
            season_fall: 'Осень',
            cat_title: 'Каталог аниме',
            cat_desc: 'Каталог аниме с русскими названиями, оценками, жанрами и описаниями. Данные AniList, Kitsu и Shikimori.',
            all_genres: 'Все жанры',
            catalog_size: 'аниме в каталоге',
        },
        en: {
            catalog: 'Anime Catalog',
            search_ph: 'Search by title…',
            found: 'Found',
            loading: 'Loading…',
            nothing: 'Nothing found',
            nothing_hint: 'Try adjusting the filters or the search query',
            error: 'Could not load',
            error_hint: 'Check your connection and try again',
            retry: 'Retry',
            filters: 'Filters',
            apply: 'Show',
            reset: 'Reset',
            clear_all: 'Clear all',
            sort: 'Sort',
            sort_popularity: 'By popularity',
            sort_score: 'By score',
            sort_score_asc: 'By score (lowest first)',
            sort_year: 'Newest first',
            sort_year_asc: 'Oldest first',
            sort_title: 'By title',
            sort_title_desc: 'By title (reverse)',
            sort_title_ru: 'By Russian title',
            sort_favourites: 'By favourites',
            sort_trending: 'By trending',
            sort_episodes: 'By episode count',
            sort_rating_count: 'By rating count',
            sort_added: 'Recently added',
            format: 'Format',
            status: 'Status',
            season: 'Season',
            genre: 'Genre',
            country: 'Country',
            year: 'Year',
            year_from: 'Year from',
            year_to: 'Year to',
            score: 'Score',
            score_from: 'from',
            score_to: 'to',
            adult: 'Rating',
            adult_any: 'Any',
            adult_safe: 'Safe only',
            adult_only: 'Adult only',
            has_ru: 'Russian title',
            has_ru_any: 'Any',
            has_ru_yes: 'Russian only',
            has_trailer: 'With trailer only',
            theme: 'Theme',
            theme_dark: 'Dark',
            theme_light: 'Light',
            account: 'Account',
            sign_in: 'Sign in',
            sign_up: 'Sign up',
            sign_out: 'Sign out',
            username: 'Username',
            email: 'Email (optional)',
            password: 'Password',
            login_or_email: 'Login or email',
            no_account: 'No account yet?',
            have_account: 'Already have an account?',
            my_list: 'My list',
            list_watching: 'Watching',
            list_planned: 'Planned',
            list_completed: 'Completed',
            list_dropped: 'Dropped',
            list_favorites: 'Favourites',
            list_all: 'All',
            list_empty: 'List is empty',
            list_empty_hint: 'Tap the star on any anime to add it here',
            add_to_list: 'Add to list',
            remove_from_list: 'Remove from list',
            added: 'Added',
            removed: 'Removed',
            sign_in_required: 'Sign in to keep your own list',
            back: 'Back',
            to_top: 'To top',
            not_found: 'Anime not found',
            not_found_hint: 'The link may be outdated or the record not loaded yet',
            description: 'Description',
            description_ru: 'Description (Shikimori)',
            description_en: 'Description (AniList)',
            genres: 'Genres',
            tags: 'Tags',
            studios: 'Studios',
            producers: 'Producers',
            licensors: 'Licensors',
            cast: 'Characters & voice actors',
            staff: 'Staff',
            related: 'Related',
            recommendations: 'Recommendations',
            external_links: 'External links',
            streaming: 'Watch online',
            trailer: 'Trailer',
            info: 'Information',
            ids: 'IDs',
            format_l: 'Format',
            status_l: 'Status',
            episodes: 'Episodes',
            duration: 'Episode length',
            per_ep: 'min/ep',
            season_l: 'Season',
            aired: 'Aired',
            ended: 'Ended',
            country_l: 'Country',
            score_l: 'Score',
            score_source: 'Score source',
            rating_count: 'Ratings',
            popularity: 'Popularity',
            favourites: 'Favourites',
            trending: 'Trending',
            synonyms: 'Synonyms',
            age_rating: 'Age rating',
            voice_actor: 'Voice',
            your_score: 'Your score',
            your_progress: 'Progress',
            episodes_watched: 'episodes',
            save: 'Save',
            cancel: 'Cancel',
            delete: 'Delete',
            notes: 'Notes',
            notes_ph: 'Personal notes…',
            watch_status: 'Status',
            season_winter: 'Winter',
            season_spring: 'Spring',
            season_summer: 'Summer',
            season_fall: 'Fall',
            cat_title: 'Anime Catalog',
            cat_desc: 'Anime catalogue with Russian titles, ratings, genres and descriptions. Data from AniList, Kitsu and Shikimori.',
            all_genres: 'All genres',
            catalog_size: 'titles in the catalog',
        },
    };

    const LS_LANG = 'anime.lang';
    const LS_THEME = 'anime.theme';
    const LS_TOKEN = 'anime.token';

    const I18n = {
        lang: localStorage.getItem(LS_LANG) || 'ru',

        t(key) {
            const table = STRINGS[this.lang] || STRINGS.ru;
            return table[key] !== undefined ? table[key] : (STRINGS.ru[key] !== undefined ? STRINGS.ru[key] : key);
        },

        setLang(lang) {
            this.lang = STRINGS[lang] ? lang : 'ru';
            localStorage.setItem(LS_LANG, this.lang);
            document.documentElement.lang = this.lang;
            document.dispatchEvent(new CustomEvent('langchange', { detail: { lang: this.lang } }));
        },

        applyStatic(root) {
            root.querySelectorAll('[data-i18n]').forEach((el) => {
                el.textContent = I18n.t(el.dataset.i18n);
            });
            root.querySelectorAll('[data-i18n-ph]').forEach((el) => {
                el.placeholder = I18n.t(el.dataset.i18nPh);
            });
            root.querySelectorAll('[data-i18n-title]').forEach((el) => {
                el.title = I18n.t(el.dataset.i18nTitle);
            });
        },
    };

    // -------------------------------------------------------------- theme

    const Theme = {
        get() {
            const saved = localStorage.getItem(LS_THEME);
            if (saved === 'light' || saved === 'dark') return saved;
            return global.matchMedia && global.matchMedia('(prefers-color-scheme: light)').matches
                ? 'light'
                : 'dark';
        },
        apply() {
            document.documentElement.dataset.theme = Theme.get();
        },
        toggle() {
            localStorage.setItem(LS_THEME, Theme.get() === 'dark' ? 'light' : 'dark');
            Theme.apply();
        },
    };

    // ---------------------------------------------------------------- api

    class ApiError extends Error {
        constructor(message, status, code) {
            super(message);
            this.status = status;
            this.code = code;
        }
    }

    const Session = {
        token: localStorage.getItem(LS_TOKEN) || null,
        user: null,

        get isAuthed() {
            return !!this.token;
        },

        set(token, user) {
            this.token = token;
            this.user = user || null;
            if (token) localStorage.setItem(LS_TOKEN, token);
            else localStorage.removeItem(LS_TOKEN);
            document.dispatchEvent(new CustomEvent('sessionchange', { detail: { authed: this.isAuthed } }));
        },

        async refresh() {
            if (!this.token) {
                this.user = null;
                return null;
            }
            try {
                this.user = await Api.get('/api/auth/me');
                return this.user;
            } catch (e) {
                // A rejected token is indistinguishable from a revoked one, so
                // drop it and let the UI fall back to anonymous.
                if (e.status === 401) this.set(null, null);
                return null;
            }
        },
    };

    const Api = {
        ApiError,

        async request(path, options) {
            const opts = Object.assign({ headers: {} }, options || {});
            if (opts.body && typeof opts.body !== 'string') {
                opts.body = JSON.stringify(opts.body);
                opts.headers['Content-Type'] = 'application/json';
            }
            if (Session.token) {
                opts.headers['Authorization'] = 'Bearer ' + Session.token;
            }
            opts.headers['Accept'] = 'application/json';

            let res;
            try {
                res = await fetch(path, opts);
            } catch (e) {
                throw new ApiError('network', 0, 'network');
            }

            if (res.status === 204) return null;

            const text = await res.text();
            let data = null;
            if (text) {
                try {
                    data = JSON.parse(text);
                } catch (e) {
                    data = null;
                }
            }

            if (!res.ok) {
                const detail = data && data.error ? data.error : null;
                throw new ApiError(
                    (detail && detail.message) || res.statusText || 'HTTP ' + res.status,
                    res.status,
                    (detail && detail.code) || 'http_' + res.status
                );
            }
            return data;
        },

        get(path, params) {
            const qs = params ? '?' + new URLSearchParams(clean(params)).toString() : '';
            return Api.request(path + qs);
        },
        post(path, body) {
            return Api.request(path, { method: 'POST', body: body || {} });
        },
        patch(path, body) {
            return Api.request(path, { method: 'PATCH', body: body || {} });
        },
        del(path) {
            return Api.request(path, { method: 'DELETE' });
        },
    };

    /** Drops empty/undefined values so query strings stay short. */
    function clean(obj) {
        const out = {};
        Object.keys(obj).forEach((k) => {
            const v = obj[k];
            if (v === '' || v === null || v === undefined || v === false) return;
            out[k] = v;
        });
        return out;
    }

    // --------------------------------------------------------------- dom

    const $ = (sel, root) => (root || document).querySelector(sel);
    const $$ = (sel, root) => Array.from((root || document).querySelectorAll(sel));

    function esc(value) {
        if (value === null || value === undefined) return '';
        return String(value).replace(/[&<>"']/g, (c) => ({
            '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
        })[c]);
    }

    /**
     * Builds an element. Children may be `null`/`undefined` — they are skipped,
     * which is what lets call sites write `cond ? el(...) : null` inline
     * instead of filtering every array by hand and eventually forgetting one.
     */
    function el(tag, attrs, children) {
        const node = document.createElement(tag);
        if (attrs) {
            Object.keys(attrs).forEach((k) => {
                if (k === 'class') node.className = attrs[k];
                else if (k === 'text') node.textContent = attrs[k];
                else if (k === 'html') node.innerHTML = attrs[k];
                else if (attrs[k] !== null && attrs[k] !== undefined) node.setAttribute(k, attrs[k]);
            });
        }
        (children || []).forEach((c) => {
            if (c === null || c === undefined || c === false) return;
            node.appendChild(typeof c === 'string' ? document.createTextNode(c) : c);
        });
        return node;
    }

    /** Cover URLs go through the server-side proxy. */
    function coverUrl(url) {
        if (!url) return null;
        if (url.startsWith('/')) return url;
        return '/api/img?u=' + encodeURIComponent(url);
    }

    function toast(message, isError) {
        let host = $('.toasts');
        if (!host) {
            host = el('div', { class: 'toasts', role: 'status', 'aria-live': 'polite' });
            document.body.appendChild(host);
        }
        const node = el('div', { class: 'toast' + (isError ? ' toast--error' : ''), text: message });
        host.appendChild(node);
        setTimeout(() => {
            node.style.opacity = '0';
            setTimeout(() => node.remove(), 200);
        }, 2600);
    }

    /**
     * Lazily loads a cover and marks the art box ready.
     *
     * The old markup assigned `onerror` and used `via.placeholder.com`, a
     * service that has been shut down for years, so a missing cover produced a
     * broken image icon instead of a placeholder.
     */
    function attachCover(art, url) {
        const src = coverUrl(url);
        if (!src) {
            art.classList.add('is-ready');
            return;
        }
        const img = el('img', { alt: '', loading: 'lazy', decoding: 'async' });
        img.addEventListener('load', () => {
            img.classList.add('is-loaded');
            art.classList.add('is-ready');
        });
        img.addEventListener('error', () => {
            // Leave the shimmer off and show an empty art box rather than a
            // broken-image glyph.
            art.classList.add('is-ready');
            img.remove();
        });
        img.src = src;
        art.appendChild(img);
    }

    /** Appends children, skipping anything falsy. */
    function add(parent) {
        for (let i = 1; i < arguments.length; i++) {
            const c = arguments[i];
            if (c === null || c === undefined || c === false) continue;
            parent.appendChild(c);
        }
        return parent;
    }

    function debounce(fn, wait) {
        let timer = null;
        return function () {
            const args = arguments;
            clearTimeout(timer);
            timer = setTimeout(() => fn.apply(this, args), wait);
        };
    }

    const STATUS_LABELS = {
        watching: { ru: 'Смотрю', en: 'Watching' },
        planned: { ru: 'Запланировано', en: 'Planned' },
        completed: { ru: 'Просмотрено', en: 'Completed' },
        dropped: { ru: 'Брошено', en: 'Dropped' },
    };

    function statusLabel(status) {
        const entry = STATUS_LABELS[status];
        if (!entry) return status;
        return I18n.lang === 'ru' ? entry.ru : entry.en;
    }

    function seasonLabel(season) {
        if (!season) return '';
        const key = 'season_' + String(season).toLowerCase();
        const value = I18n.t(key);
        return value === key ? season : value;
    }

    function formatDate(value) {
        if (!value) return '';
        // Accepts YYYY, YYYY-MM and YYYY-MM-DD from the different sources.
        return String(value).replace(/-/g, '.');
    }

    global.App = {
        I18n, Theme, Api, ApiError, Session,
        $, $$, esc, el, add, toast, attachCover, debounce, coverUrl, clean,
        statusLabel, seasonLabel, formatDate,
    };
})(window);
