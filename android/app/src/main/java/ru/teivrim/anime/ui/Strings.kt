package ru.teivrim.anime.ui

import ru.teivrim.anime.data.AppLanguage

/**
 * Interface strings.
 *
 * Resolved in code rather than through `res/values/strings.xml` so that a
 * missing translation falls back to the base language instead of showing a raw
 * resource id, and so the RU/EN switch is a single value rather than a
 * locale-reconfiguration dance. It also keeps the two languages side by side,
 * which makes a missing translation obvious in review.
 */
class Strings(private val lang: AppLanguage) {

    private fun ru(ru: String, en: String): String = if (lang == AppLanguage.Russian) ru else en

    val catalog: String get() = ru("Каталог аниме", "Anime Catalog")
    val search: String get() = ru("Поиск", "Search")
    val searchHint: String get() = ru("Поиск по названию…", "Search by title…")
    val cancel: String get() = ru("Отмена", "Cancel")
    val apply: String get() = ru("Применить", "Apply")
    val reset: String get() = ru("Сбросить", "Reset")
    val retry: String get() = ru("Повторить", "Retry")
    val back: String get() = ru("Назад", "Back")
    val close: String get() = ru("Закрыть", "Close")
    val save: String get() = ru("Сохранить", "Save")
    val delete: String get() = ru("Удалить", "Delete")
    val loading: String get() = ru("Загрузка…", "Loading…")
    val found: String get() = ru("Найдено", "Found")
    val nothing: String get() = ru("Ничего не найдено", "Nothing found")
    val nothingHint: String get() = ru(
        "Попробуйте изменить фильтры или запрос",
        "Try adjusting the filters or the query",
    )
    val error: String get() = ru("Не удалось загрузить", "Could not load")
    val errorHint: String get() = ru(
        "Проверьте подключение к серверу",
        "Check the connection to the server",
    )
    val offline: String get() = ru("Нет связи с сервером", "Cannot reach the server")
    val rateLimited: String get() = ru("Слишком много запросов", "Too many requests")
    val notFound: String get() = ru("Аниме не найдено", "Anime not found")
    val notFoundHint: String get() = ru(
        "Ссылка устарела или запись ещё не загружена",
        "The link is outdated or the record is not loaded yet",
    )

    val filters: String get() = ru("Фильтры", "Filters")
    val sort: String get() = ru("Сортировка", "Sort")
    val format: String get() = ru("Формат", "Format")
    val status: String get() = ru("Статус", "Status")
    val season: String get() = ru("Сезон", "Season")
    val genre: String get() = ru("Жанр", "Genre")
    val country: String get() = ru("Страна", "Country")
    val year: String get() = ru("Год", "Year")
    val yearFrom: String get() = ru("Год с", "Year from")
    val yearTo: String get() = ru("Год по", "Year to")
    val score: String get() = ru("Оценка", "Score")
    val scoreFrom: String get() = ru("от", "from")
    val scoreTo: String get() = ru("до", "to")
    val adult: String get() = ru("Возраст", "Rating")
    val adultAny: String get() = ru("Любой", "Any")
    val adultSafe: String get() = ru("Без 18+", "Safe only")
    val adultOnly: String get() = ru("Только 18+", "Adult only")
    val hasRussian: String get() = ru("Русское название", "Russian title")
    val hasRussianAny: String get() = ru("Любой", "Any")
    val hasRussianOnly: String get() = ru("Только с русским", "Russian only")
    val hasTrailer: String get() = ru("Только с трейлером", "With trailer only")
    val activeFilters: String get() = ru("активных фильтров", "filters active")

    val episodes: String get() = ru("эпизодов", "episodes")
    val episode: String get() = ru("эпизод", "episode")
    val perEpisode: String get() = ru("мин/эп", "min/ep")
    val ratingCount: String get() = ru("оценок", "ratings")
    val popularity: String get() = ru("популярность", "popularity")
    val favourites: String get() = ru("в избранном", "favourites")
    val trending: String get() = ru("тренд", "trending")
    val adultBadge: String get() = "18+"

    val description: String get() = ru("Описание", "Description")
    val descriptionRu: String get() = ru("Описание (Shikimori)", "Description (Shikimori)")
    val descriptionEn: String get() = ru("Описание (AniList)", "Description (AniList)")
    val genres: String get() = ru("Жанры", "Genres")
    val tags: String get() = ru("Теги", "Tags")
    val studios: String get() = ru("Студии", "Studios")
    val producers: String get() = ru("Продюсеры", "Producers")
    val licensors: String get() = ru("Лицензиары", "Licensors")
    val synonyms: String get() = ru("Синонимы", "Synonyms")
    val cast: String get() = ru("Персонажи и озвучка", "Characters & voice actors")
    val staff: String get() = ru("Создатели", "Staff")
    val voiceActor: String get() = ru("Озвучка", "Voice")
    val related: String get() = ru("Связанные работы", "Related")
    val recommendations: String get() = ru("Похожее", "Recommendations")
    val externalLinks: String get() = ru("Внешние ссылки", "External links")
    val linkFailed: String get() = ru(
        "Не удалось открыть ссылку",
        "Could not open the link",
    )
    val streaming: String get() = ru("Смотреть онлайн", "Watch online")
    val trailer: String get() = ru("Трейлер", "Trailer")
    val information: String get() = ru("Информация", "Information")
    val ids: String get() = ru("ID", "IDs")
    val aired: String get() = ru("Дата выхода", "Aired")
    val ended: String get() = ru("Дата окончания", "Ended")
    val scoreSource: String get() = ru("Источник оценки", "Score source")
    val ageRating: String get() = ru("Возрастной рейтинг", "Age rating")

    val myList: String get() = ru("Мой список", "My list")
    val addToList: String get() = ru("В список", "Add to list")
    val editList: String get() = ru("Изменить запись", "Edit entry")
    val removeFromList: String get() = ru("Убрать из списка", "Remove from list")
    val added: String get() = ru("Добавлено", "Added")
    val removed: String get() = ru("Убрано", "Removed")
    val listEmpty: String get() = ru("Список пуст", "The list is empty")
    val listEmptyHint: String get() = ru(
        "Нажмите на звездочку у любого аниме",
        "Tap the star on any anime to add it here",
    )
    val signInRequired: String get() = ru(
        "Войдите, чтобы вести свой список",
        "Sign in to keep your own list",
    )
    val watchStatus: String get() = ru("Статус", "Status")
    val yourScore: String get() = ru("Ваша оценка", "Your score")
    val yourProgress: String get() = ru("Прогресс", "Progress")
    val notes: String get() = ru("Заметки", "Notes")
    val notesHint: String get() = ru("Личные заметки…", "Personal notes…")
    val markFavorite: String get() = ru("В избранное", "Favourite")

    val account: String get() = ru("Аккаунт", "Account")
    val signIn: String get() = ru("Войти", "Sign in")
    val signUp: String get() = ru("Регистрация", "Sign up")
    val signOut: String get() = ru("Выйти", "Sign out")
    val username: String get() = ru("Имя пользователя", "Username")
    val email: String get() = ru("E-mail", "Email")
    val emailOptional: String get() = ru("E-mail (необязательно)", "Email (optional)")
    val password: String get() = ru("Пароль", "Password")
    val loginOrEmail: String get() = ru("Логин или e-mail", "Login or email")
    val passwordHint: String get() = ru(
        "Не короче 8 символов",
        "At least 8 characters",
    )
    val passwordMismatch: String get() = ru("Пароли не совпадают", "Passwords do not match")
    val usernameTooShort: String get() = ru(
        "Имя пользователя: от 3 до 32 символов",
        "Username: 3 to 32 characters",
    )
    val noAccount: String get() = ru("Нет аккаунта?", "No account yet?")
    val haveAccount: String get() = ru("Уже есть аккаунт?", "Already have an account?")
    val settings: String get() = ru("Настройки", "Settings")
    val language: String get() = ru("Язык", "Language")
    val russianTitles: String get() = ru(
        "Сначала русские названия",
        "Prefer Russian titles",
    )
    val theme: String get() = ru("Тема", "Theme")
    val themeSystem: String get() = ru("Как в системе", "Follow system")
    val themeLight: String get() = ru("Светлая", "Light")
    val themeDark: String get() = ru("Тёмная", "Dark")
    val server: String get() = ru("Сервер", "Server")
    val version: String get() = ru("Версия", "Version")

    val watchStatusLabel: (String) -> String = { api ->
        when (api) {
            "watching" -> ru("Смотрю", "Watching")
            "planned" -> ru("Запланировано", "Planned")
            "completed" -> ru("Просмотрено", "Completed")
            "dropped" -> ru("Брошено", "Dropped")
            else -> api
        }
    }

    val statusLabel: (String) -> String = { api ->
        when (api) {
            "FINISHED" -> ru("Завершено", "Finished")
            "RELEASING" -> ru("Выпускается", "Airing")
            "NOT_YET_RELEASED" -> ru("Ещё не вышел", "Not yet released")
            "HIATUS" -> ru("Приостановлено", "On hiatus")
            "CANCELLED" -> ru("Отменено", "Cancelled")
            else -> api
        }
    }

    val seasonLabel: (String) -> String = { api ->
        when (api.lowercase()) {
            "winter" -> ru("Зима", "Winter")
            "spring" -> ru("Весна", "Spring")
            "summer" -> ru("Лето", "Summer")
            "fall", "autumn" -> ru("Осень", "Fall")
            else -> api
        }
    }

    /** Sort options in display order, as (api value, label). */
    fun sorts(): List<Pair<String, String>> = listOf(
        "popularity" to ru("По популярности", "By popularity"),
        "score" to ru("По оценке", "By score"),
        "score_asc" to ru("По оценке, сначала низкие", "By score, lowest first"),
        "rating_count" to ru("По числу оценок", "By rating count"),
        "favourites" to ru("По избранному", "By favourites"),
        "trending" to ru("По тренду", "By trending"),
        "year" to ru("Сначала новые", "Newest first"),
        "year_asc" to ru("Сначала старые", "Oldest first"),
        "title" to ru("По названию", "By title"),
        "title_desc" to ru("По названию, наоборот", "By title, reverse"),
        "title_ru" to ru("По русскому названию", "By Russian title"),
        "episodes" to ru("По числу эпизодов", "By episode count"),
        "duration" to ru("По длительности", "By episode length"),
        "added" to ru("Сначала добавленные", "Recently added"),
    )
}
