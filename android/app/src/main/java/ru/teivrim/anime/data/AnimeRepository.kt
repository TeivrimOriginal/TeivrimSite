package ru.teivrim.anime.data

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import okhttp3.HttpUrl.Companion.toHttpUrl
import java.net.URLEncoder

/**
 * Everything the UI needs from the network, in one place.
 *
 * Screens talk to this and never to [ApiClient] directly, so a change to the
 * wire format or the error handling lives in one file.
 */
class AnimeRepository(
    private val api: ApiClient,
    private val session: SessionStore,
) {
    val signedIn: Boolean get() = session.isSignedIn

    // ------------------------------------------------------------- catalogue

    suspend fun list(
        filter: CatalogFilter,
        page: Int,
        perPage: Int = 48,
    ): Paged<AnimeSummary> {
        val query = buildString {
            filter.toQuery().forEach { (k, v) -> appendParam(k, v) }
            appendParam("page", page.toString())
            appendParam("per_page", perPage.toString())
        }
        return api.get("api/anime?$query")
    }

    suspend fun detail(uid: String): AnimeDetail = api.get("api/anime/${uid.urlPath()}")

    suspend fun filters(): Filters = api.get("api/filters")

    suspend fun genres(): List<Genre> = api.get<GenresResponse>("api/genres?category=genre&min_count=3").genres

    suspend fun suggest(term: String, limit: Int = 8): List<Suggestion> =
        api.get("api/search/suggest?q=${term.urlParam()}&limit=$limit")

    // ------------------------------------------------------------------ auth

    suspend fun register(username: String, email: String?, password: String): PublicUser {
        val payload = RegisterPayload(username, email?.takeIf { it.isNotBlank() }, password)
        val response: AuthResponse = api.post("api/auth/register", api.encode(payload))
        session.setToken(response.token)
        return response.user
    }

    suspend fun login(login: String, password: String): PublicUser {
        val response: AuthResponse = api.post("api/auth/login", api.encode(LoginPayload(login, password)))
        session.setToken(response.token)
        return response.user
    }

    suspend fun me(): PublicUser = api.get("api/auth/me")

    suspend fun logout() {
        // Clear locally first: whether the network call succeeds must not
        // decide whether the user stays signed in on this device.
        session.setToken(null)
        runCatching { api.post<Unit>("api/auth/logout") }
    }

    /** Confirms a stored token is still good; clears it if not. */
    suspend fun refreshSession(): PublicUser? = try {
        me()
    } catch (e: ApiException.Unauthorized) {
        null
    } catch (e: Exception) {
        null
    }

    // -------------------------------------------------------------- watchlist

    suspend fun favorite(uid: String, favorite: Boolean) {
        val payload = UpsertPayload(uid = uid, isFavorite = favorite)
        api.post<LibraryEntry>("api/favorites", api.encode(payload))
    }

    suspend fun saveEntry(payload: UpsertPayload): LibraryEntry =
        api.post("api/favorites", api.encode(payload))

    suspend fun removeFromList(uid: String) {
        api.delete("api/favorites/${uid.urlPath()}")
    }

    suspend fun watchlist(
        status: WatchStatus? = null,
        favoritesOnly: Boolean = false,
        limit: Int = 200,
    ): List<ListEntry> {
        val query = buildString {
            status?.let { appendParam("status", it.api) }
            if (favoritesOnly) appendParam("favorites", "1")
            appendParam("limit", limit.toString())
        }
        return api.get("api/favorites?$query")
    }

    suspend fun counts(): FavoritesCounts = api.get("api/favorites/counts")

    // ------------------------------------------------------------------ misc

    fun coverUrl(raw: String?): String? = api.proxied(raw)

    private fun StringBuilder.appendParam(name: String, value: String) {
        if (isNotEmpty()) append('&')
        append(name).append('=').append(URLEncoder.encode(value, "UTF-8"))
    }
}

/** uid looks like `al:16498`; a colon is legal in a path segment but encoding
 *  it keeps the URL unambiguous for proxies in the middle. */
private fun String.urlPath(): String = URLEncoder.encode(this, "UTF-8").replace("+", "%20")

private fun String.urlParam(): String = URLEncoder.encode(this, "UTF-8")

@Serializable
data class RegisterPayload(
    val username: String,
    val email: String? = null,
    val password: String,
)

@Serializable
data class LoginPayload(
    val login: String,
    val password: String,
)

@Serializable
data class UpsertPayload(
    val uid: String,
    val status: String? = null,
    @kotlinx.serialization.SerialName("is_favorite") val isFavorite: Boolean? = null,
    val score: Int? = null,
    val progress: Int? = null,
    val notes: String? = null,
)
