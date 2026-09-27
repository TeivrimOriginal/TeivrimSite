package ru.teivrim.anime.data

import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.Interceptor
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import ru.teivrim.anime.BuildConfig
import java.io.IOException
import java.util.concurrent.TimeUnit

private const val TAG = "AnimeApi"

/** A failure with enough structure for the UI to react differently per case. */
sealed class ApiException(message: String, cause: Throwable? = null) : Exception(message, cause) {
    /** No connectivity, DNS failure, or the server never answered. */
    class Network(cause: Throwable) : ApiException("network unreachable", cause)

    /** 401 — the token is missing, expired or revoked. */
    class Unauthorized : ApiException("unauthorized")

    /** 429 — the server rate limiter rejected the call. */
    class RateLimited(val retryAfterSec: Int) : ApiException("rate limited")

    /** 5xx. */
    class Server(val code: Int, message: String) : ApiException(message)

    /** 4xx with a message the server chose to expose. */
    class Client(val code: Int, override val message: String) : ApiException(message)
}

/**
 * Thin HTTP client for the catalogue API.
 *
 * Hand-rolled rather than Retrofit: the surface is a dozen endpoints with one
 * query shape, and this keeps the dependency list (and the APK size) smaller.
 * OkHttp is already required for connection pooling, so the extra cost of not
 * using Retrofit is close to zero.
 */
class ApiClient(
    private val baseUrl: String,
    private val tokenProvider: () -> String?,
    private val onUnauthorized: () -> Unit,
) {
    private val json = Json {
        ignoreUnknownKeys = true
        coerceInputValues = true
        explicitNulls = false
    }

    /**
     * Encoding for request bodies.
     *
     * `explicitNulls = true` is the opposite of the decoding setting on purpose:
     * the watchlist endpoint uses PATCH semantics where "field absent" means
     * *leave as is* and "field is null" means *clear it*. Dropping nulls on the
     * way out would make a cleared score impossible to save.
     */
    private val requestJson = Json {
        encodeDefaults = true
        explicitNulls = true
    }

    // Exposed for the inline encode/decode helpers, which are compiled into
    // caller modules and may only reach members that are at least internal.
    @PublishedApi
    internal val encoder: Json get() = requestJson

    @PublishedApi
    internal val decoder: Json get() = json

    // Declared before `client`: property initialisers run top to bottom, so an
    // interceptor referenced from the builder has to already exist or the client
    // captures null and the first request throws.
    private val authInterceptor = Interceptor { chain ->
        val token = tokenProvider()
        val request = if (token.isNullOrBlank()) {
            chain.request()
        } else {
            chain.request().newBuilder()
                .header("Authorization", "Bearer $token")
                .build()
        }
        chain.proceed(request)
    }

    private val client = OkHttpClient.Builder()
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .writeTimeout(30, TimeUnit.SECONDS)
        .retryOnConnectionFailure(true)
        .addInterceptor(authInterceptor)
        .addInterceptor { chain ->
            val req = chain.request()
            if (!BuildConfig.DEBUG) return@addInterceptor chain.proceed(req)
            // Log bodies only in debug builds: they contain the bearer token.
            chain.proceed(req).also { res ->
                Log.d(TAG, "${req.method} ${req.url.encodedPath} -> ${res.code}")
            }
        }
        .build()

    // ------------------------------------------------------------------ URLs

    /**
     * Builds an absolute URL under [baseUrl].
     *
     * [path] may already carry a query string, which is why it is parsed and
     * re-encoded rather than concatenated: a title filter like `q=a b` has to
     * survive the round trip.
     *
     * `@PublishedApi internal` rather than private because [get], [post] and
     * [patch] are inline: an inline function is compiled into the caller's
     * module and may only reach members that are at least internal.
     */
    @PublishedApi
    internal fun buildUrl(path: String): HttpUrl {
        val root = if (baseUrl.endsWith("/")) baseUrl else "$baseUrl/"
        val (rawPath, rawQuery) = path.split('?', limit = 2).let {
            it[0] to it.getOrNull(1).orEmpty()
        }
        val builder = root.toHttpUrl().newBuilder()
        rawPath.trim('/').split('/').forEach { segment ->
            if (segment.isNotEmpty()) builder.addPathSegment(segment)
        }
        if (rawQuery.isNotEmpty()) {
            rawQuery.split('&').forEach { pair ->
                if (pair.isBlank()) return@forEach
                val name = pair.substringBefore('=')
                val value = pair.substringAfter('=', "")
                builder.addEncodedQueryParameter(name, value)
            }
        }
        return builder.build()
    }

    /**
     * Cover and person-image URLs come straight from upstream CDNs, so they are
     * passed through the server's proxy. That gives one cacheable endpoint, an
     * allow-list, and a generated placeholder when an image is missing.
     */
    fun proxied(raw: String?): String? {
        if (raw.isNullOrBlank()) return null
        if (!raw.startsWith("http://") && !raw.startsWith("https://")) return raw
        return buildUrl("api/img").newBuilder()
            .addQueryParameter("u", raw)
            .build()
            .toString()
    }

    // -------------------------------------------------------------- requests

    suspend inline fun <reified T> get(path: String): T = execute(
        Request.Builder().url(buildUrl(path)).get().build(),
    )

    suspend inline fun <reified T> post(path: String, body: String = "{}"): T = execute(
        Request.Builder().url(buildUrl(path))
            .post(body.toRequestBody(JSON_MEDIA))
            .build(),
    )

    suspend inline fun <reified T> patch(path: String, body: String): T = execute(
        Request.Builder().url(buildUrl(path))
            .patch(body.toRequestBody(JSON_MEDIA))
            .build(),
    )

    suspend fun delete(path: String) {
        val request = Request.Builder().url(buildUrl(path)).delete().build()
        val response = runCatching { client.newCall(request).execute() }
            .getOrElse { throw ApiException.Network(it) }
        response.use {
            if (it.code == 404) return // already gone
            if (!it.isSuccessful) throw errorFor(it)
        }
    }

    /**
     * Performs the call on the IO dispatcher and decodes the body.
     *
     * The `inline reified` signature is what lets callers write
     * `api.get<AnimeDetail>(...)` without an explicit type token.
     */
    suspend inline fun <reified T> execute(request: Request): T =
        decode(executeRaw(request))

    suspend fun executeRaw(request: Request): String = withContext(Dispatchers.IO) {
        val response = try {
            client.newCall(request).execute()
        } catch (e: IOException) {
            throw ApiException.Network(e)
        }
        response.use {
            val body = it.body?.string().orEmpty()
            if (it.code == 401) {
                // A rejected token is always revoked or expired; drop it so the
                // UI falls back to anonymous instead of retrying forever.
                onUnauthorized()
                throw ApiException.Unauthorized()
            }
            if (it.code == 429) {
                throw ApiException.RateLimited(it.header("Retry-After")?.toIntOrNull() ?: 5)
            }
            if (!it.isSuccessful) throw errorFor(it, body)
            body
        }
    }

    private fun errorFor(response: Response, body: String? = null): ApiException {
        val text = body ?: runCatching { response.body?.string().orEmpty() }.getOrDefault("")
        val parsed = runCatching { json.decodeFromString<ApiErrorBody>(text) }.getOrNull()
        val message = parsed?.error?.message?.takeIf { it.isNotBlank() }
            ?: "HTTP ${response.code}"
        return when {
            response.code in 500..599 -> ApiException.Server(response.code, message)
            else -> ApiException.Client(response.code, message)
        }
    }

    /** Serialises a request body. Nulls are written so the server can tell
     *  "clear this" from "leave it alone"; see `requestJson`. */
    inline fun <reified T> encode(value: T): String = encoder.encodeToString(value)

    @PublishedApi
    internal inline fun <reified T> decode(text: String): T = decoder.decodeFromString(text)

    companion object {
        val JSON_MEDIA = "application/json; charset=utf-8".toMediaType()
    }
}
