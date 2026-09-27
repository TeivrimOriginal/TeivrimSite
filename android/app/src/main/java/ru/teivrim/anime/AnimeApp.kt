package ru.teivrim.anime

import android.app.Application
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import ru.teivrim.anime.data.AnimeRepository
import ru.teivrim.anime.data.ApiClient
import ru.teivrim.anime.data.SessionStore

/**
 * Manual dependency container.
 *
 * Hilt would be the usual choice, but the graph is three objects deep and
 * adding an annotation processor for it costs build time and APK size that
 * nothing here needs.
 */
class AnimeApp : Application() {

    lateinit var session: SessionStore
        private set
    lateinit var api: ApiClient
        private set
    lateinit var repository: AnimeRepository
        private set

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    override fun onCreate() {
        super.onCreate()
        instance = this

        session = SessionStore(this)
        api = ApiClient(
            baseUrl = BuildConfig.API_BASE_URL,
            tokenProvider = { session.cachedToken },
            onUnauthorized = {
                // Fire and forget: the interceptor runs on a network thread, so
                // this must not block.
                scope.launch { session.setToken(null) }
                Log.i("AnimeApp", "session token rejected, signing out")
            },
        )
        repository = AnimeRepository(api, session)

        scope.launch {
            // The OkHttp interceptor reads the token synchronously, so it has
            // to be in memory before the first request goes out.
            session.loadCached()
        }
    }

    /** Image URLs are proxied by the server; a rebuild here means one origin. */
    fun coverUrl(raw: String?): String? = repository.coverUrl(raw)

    companion object {
        lateinit var instance: AnimeApp
            private set
    }
}
