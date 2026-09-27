package ru.teivrim.anime.data

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Wire types for the Anime DB API.
 *
 * These mirror `src/models.rs` exactly. Every field the server may omit is
 * nullable with a default, so adding a field server-side cannot crash an
 * installed client — kotlinx.serialization only fails on a *missing required*
 * field, and nothing here is required except the ones the server always sends.
 */

@Serializable
data class Paged<T>(
    val items: List<T> = emptyList(),
    val page: Int = 1,
    val perPage: Int = 48,
    val total: Long = 0,
    val totalPages: Int = 0,
    val hasMore: Boolean = false,
)

@Serializable
data class Genre(
    val id: Long = 0,
    val slug: String = "",
    val name: String = "",
    val nameRu: String? = null,
    val category: String? = null,
    val count: Long? = null,
) {
    /** The label to show, preferring the Russian name when present. */
    fun display(): String = nameRu ?: name
}

/**
 * The fields a grid card needs.
 *
 * Both a catalogue row and a watchlist row carry these, but the watchlist row
 * also carries the viewer's own fields, so they cannot be the same type. An
 * interface lets the card accept either without copying the data or rebuilding
 * a summary just to render it.
 */
interface CardData {
    val uid: String
    val title: String
    val titleRomaji: String?
    val titleEnglish: String?
    val titleRussian: String?
    val titleNative: String?
    val cover: String?
    val score: Int?
    val format: String?
    val episodes: Int?
    val year: Int?

    /**
     * Title in the requested language, with a sensible fallback chain.
     * `title` already comes from the server resolved that way, so this only has
     * to cover the alternate-title line.
     */
    fun secondaryTitle(russianFirst: Boolean): String? {
        val main = title
        val candidates = if (russianFirst) {
            listOfNotNull(titleRomaji, titleEnglish, titleNative)
        } else {
            listOfNotNull(titleRussian, titleRomaji, titleNative)
        }
        return candidates.firstOrNull { it.isNotBlank() && it != main }
    }
}

@Serializable
data class AnimeSummary(
    override val uid: String = "",
    override val title: String = "",
    override val titleRomaji: String? = null,
    override val titleEnglish: String? = null,
    override val titleRussian: String? = null,
    override val titleNative: String? = null,
    override val cover: String? = null,
    val coverColor: String? = null,
    override val score: Int? = null,
    val scoreSource: String? = null,
    override val format: String? = null,
    val status: String? = null,
    override val episodes: Int? = null,
    val duration: Int? = null,
    override val year: Int? = null,
    val season: String? = null,
    val seasonYear: Int? = null,
    val country: String? = null,
    val isAdult: Boolean = false,
    val genres: List<Genre> = emptyList(),
) : CardData

@Serializable
data class SourceIds(
    val anilist: Long? = null,
    val kitsu: Long? = null,
    val shikimori: Long? = null,
    val mal: Long? = null,
)

@Serializable
data class NamedRef(
    val name: String = "",
    val nameRu: String? = null,
    val isMain: Boolean? = null,
) {
    fun display(): String = nameRu ?: name
}

@Serializable
data class Relation(
    val relation: String = "",
    val uid: String = "",
    val title: String = "",
    val format: String? = null,
    val status: String? = null,
    val cover: String? = null,
)

@Serializable
data class ExternalLink(
    val site: String = "",
    val url: String = "",
    val type: String? = null,
)

@Serializable
data class StreamingLink(
    val site: String = "",
    val url: String = "",
    val title: String? = null,
    val thumbnail: String? = null,
)

@Serializable
data class Recommendation(
    val uid: String = "",
    val title: String = "",
    val rating: Int? = null,
    val format: String? = null,
    val cover: String? = null,
)

@Serializable
data class Tag(
    val name: String = "",
    val rank: Int? = null,
    val spoiler: Boolean? = null,
)

@Serializable
data class Person(
    val name: String = "",
    val image: String? = null,
    val role: String? = null,
    val voiceActor: String? = null,
    val positions: List<String>? = null,
)

@Serializable
data class Trailer(
    val site: String = "",
    val id: String = "",
    val url: String = "",
    val thumbnail: String? = null,
)

@Serializable
data class LibraryEntry(
    val uid: String = "",
    /** watching | planned | completed | dropped */
    val status: String = "planned",
    val isFavorite: Boolean = false,
    val score: Int? = null,
    val progress: Int? = null,
    val episodes: Int? = null,
    val notes: String? = null,
    val updatedAt: Long = 0,
)

@Serializable
data class AnimeDetail(
    val uid: String = "",
    val ids: SourceIds = SourceIds(),
    val titleRomaji: String? = null,
    val titleEnglish: String? = null,
    val titleNative: String? = null,
    val titleRussian: String? = null,
    val synonyms: List<String> = emptyList(),
    val format: String? = null,
    val status: String? = null,
    val description: String? = null,
    val descriptionRu: String? = null,
    val duration: Int? = null,
    val episodes: Int? = null,
    val chapters: Int? = null,
    val volumes: Int? = null,
    val country: String? = null,
    val isAdult: Boolean = false,
    val isLicensed: Boolean? = null,
    val season: String? = null,
    val seasonYear: Int? = null,
    val startDate: String? = null,
    val endDate: String? = null,
    val score: Int? = null,
    val scoreSource: String? = null,
    val meanScore: Int? = null,
    val popularity: Int? = null,
    val favourites: Int? = null,
    val trending: Int? = null,
    val ratingCount: Int? = null,
    val coverSmall: String? = null,
    val coverMedium: String? = null,
    val coverLarge: String? = null,
    val coverColor: String? = null,
    val banner: String? = null,
    val trailer: Trailer? = null,
    val genres: List<Genre> = emptyList(),
    val tags: List<Tag> = emptyList(),
    val studios: List<NamedRef> = emptyList(),
    val producers: List<NamedRef> = emptyList(),
    val licensors: List<NamedRef> = emptyList(),
    val ageRating: String? = null,
    val relations: List<Relation> = emptyList(),
    val externalLinks: List<ExternalLink> = emptyList(),
    val streaming: List<StreamingLink> = emptyList(),
    val recommendations: List<Recommendation> = emptyList(),
    val characters: List<Person> = emptyList(),
    val staff: List<Person> = emptyList(),
    val library: LibraryEntry? = null,
    val updatedAt: Long? = null,
) {
    fun title(russianFirst: Boolean): String = when {
        russianFirst && !titleRussian.isNullOrBlank() -> titleRussian
        !titleEnglish.isNullOrBlank() -> titleEnglish
        !titleRomaji.isNullOrBlank() -> titleRomaji
        !titleNative.isNullOrBlank() -> titleNative
        else -> "?"
    }

    val cover: String? get() = coverLarge ?: coverMedium ?: coverSmall
}

/** A watchlist row: catalogue summary plus the user's own fields. */
@Serializable
data class ListEntry(
    override val uid: String = "",
    override val title: String = "",
    override val titleRomaji: String? = null,
    override val titleEnglish: String? = null,
    override val titleRussian: String? = null,
    override val titleNative: String? = null,
    override val cover: String? = null,
    val coverColor: String? = null,
    override val score: Int? = null,
    val scoreSource: String? = null,
    override val format: String? = null,
    val status: String? = null,
    override val episodes: Int? = null,
    val duration: Int? = null,
    override val year: Int? = null,
    val season: String? = null,
    val seasonYear: Int? = null,
    val country: String? = null,
    val isAdult: Boolean = false,
    val genres: List<Genre> = emptyList(),
    val library: LibraryEntry,
) : CardData

@Serializable
data class Suggestion(
    val uid: String = "",
    val title: String = "",
    val titleRomaji: String? = null,
    val titleEnglish: String? = null,
    val titleRussian: String? = null,
    val titleNative: String? = null,
    val cover: String? = null,
    val popularity: Int? = null,
    val score: Int? = null,
)

@Serializable
data class AuthResponse(
    val token: String = "",
    val expiresAt: Long = 0,
    val user: PublicUser,
)

@Serializable
data class PublicUser(
    val id: Long = 0,
    val username: String = "",
    val email: String? = null,
    val createdAt: Long = 0,
)

@Serializable
data class Filters(
    val formats: List<String> = emptyList(),
    val statuses: List<String> = emptyList(),
    val seasons: List<String> = emptyList(),
    val countries: List<String> = emptyList(),
    val yearMin: Int? = null,
    val yearMax: Int? = null,
)

@Serializable
data class GenresResponse(val genres: List<Genre> = emptyList())

@Serializable
data class FavoritesCounts(
    val watching: Int = 0,
    val planned: Int = 0,
    val completed: Int = 0,
    val dropped: Int = 0,
    val favorites: Int = 0,
    val total: Int = 0,
)

@Serializable
data class ApiErrorBody(val error: ApiErrorDetail = ApiErrorDetail())

@Serializable
data class ApiErrorDetail(
    val code: String = "",
    val message: String = "",
)

/** Filters as held by the catalogue screen. */
data class CatalogFilter(
    val q: String = "",
    val sort: String = "popularity",
    val format: String = "",
    val status: String = "",
    val season: String = "",
    val genre: String = "",
    val country: String = "",
    val yearFrom: String = "",
    val yearTo: String = "",
    val scoreFrom: String = "",
    val scoreTo: String = "",
    val adult: String = "",
    val hasRussian: String = "",
    val hasTrailer: String = "",
) {
    /** Only non-default values, as query parameters. */
    fun toQuery(): Map<String, String> = buildMap {
        if (q.isNotBlank()) put("q", q)
        if (sort.isNotBlank() && sort != "popularity") put("sort", sort)
        if (format.isNotBlank()) put("format", format)
        if (status.isNotBlank()) put("status", status)
        if (season.isNotBlank()) put("season", season)
        if (genre.isNotBlank()) put("genre", genre)
        if (country.isNotBlank()) put("country", country)
        if (yearFrom.isNotBlank()) put("year_from", yearFrom)
        if (yearTo.isNotBlank()) put("year_to", yearTo)
        if (scoreFrom.isNotBlank()) put("score_from", scoreFrom)
        if (scoreTo.isNotBlank()) put("score_to", scoreTo)
        if (adult.isNotBlank()) put("adult", adult)
        if (hasRussian.isNotBlank()) put("has_ru", hasRussian)
        if (hasTrailer.isNotBlank()) put("has_trailer", hasTrailer)
    }

    fun activeCount(): Int = toQuery().count { (k, _) -> k != "sort" }

    val isDefault: Boolean get() = toQuery().isEmpty()

    /**
     * Clears the filter addressed by its query-parameter name.
     *
     * The active-filter row renders one removable chip per key, so the removal
     * has to be expressible as a single key rather than as a whole new filter
     * object built at the call site.
     */
    fun without(key: String): CatalogFilter = when (key) {
        "q" -> copy(q = "")
        "sort" -> copy(sort = "popularity")
        "format" -> copy(format = "")
        "status" -> copy(status = "")
        "season" -> copy(season = "")
        "genre" -> copy(genre = "")
        "country" -> copy(country = "")
        "year" -> copy(yearFrom = "", yearTo = "")
        "score" -> copy(scoreFrom = "", scoreTo = "")
        "adult" -> copy(adult = "")
        "has_ru" -> copy(hasRussian = "")
        "has_trailer" -> copy(hasTrailer = "")
        else -> this
    }
}
