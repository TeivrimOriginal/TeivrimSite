package ru.teivrim.anime.data

import android.content.Context
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.booleanPreferencesKey
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map

private val Context.dataStore: DataStore<Preferences> by preferencesDataStore(name = "anime")

/** Watchlist statuses, in the order the UI shows them. */
enum class WatchStatus(val api: String) {
    Watching("watching"),
    Planned("planned"),
    Completed("completed"),
    Dropped("dropped");

    companion object {
        fun fromApi(value: String?): WatchStatus =
            entries.firstOrNull { it.api == value } ?: Planned
    }
}

enum class AppLanguage(val api: String) {
    Russian("ru"),
    English("en");

    companion object {
        fun fromApi(value: String?): AppLanguage =
            entries.firstOrNull { it.api == value } ?: Russian
    }
}

enum class ThemeMode(val api: String) {
    System("system"),
    Light("light"),
    Dark("dark");

    companion object {
        fun fromApi(value: String?): ThemeMode =
            entries.firstOrNull { it.api == value } ?: System
    }
}

/**
 * Local preferences: the session token, the interface language, and the theme.
 *
 * The token is cached in memory as well as on disk because the OkHttp
 * interceptor needs it synchronously on every request, and reading DataStore
 * from there would mean blocking a network thread.
 */
class SessionStore(private val context: Context) {

    @Volatile
    var cachedToken: String? = null
        private set

    val tokenFlow: Flow<String?> = context.dataStore.data.map { it[KEY_TOKEN] }

    val languageFlow: Flow<AppLanguage> = context.dataStore.data.map {
        AppLanguage.fromApi(it[KEY_LANGUAGE])
    }

    val themeFlow: Flow<ThemeMode> = context.dataStore.data.map {
        ThemeMode.fromApi(it[KEY_THEME])
    }

    val russianFirstFlow: Flow<Boolean> = context.dataStore.data.map {
        // Defaults to Russian because that is the primary audience of the app.
        it[KEY_RUSSIAN_FIRST] ?: true
    }

    suspend fun loadCached() {
        cachedToken = context.dataStore.data.map { it[KEY_TOKEN] }.first()
    }
    suspend fun setToken(token: String?) {
        cachedToken = token
        context.dataStore.edit { prefs ->
            if (token.isNullOrBlank()) prefs.remove(KEY_TOKEN) else prefs[KEY_TOKEN] = token
        }
    }

    suspend fun setLanguage(language: AppLanguage) {
        context.dataStore.edit { it[KEY_LANGUAGE] = language.api }
    }

    suspend fun setTheme(mode: ThemeMode) {
        context.dataStore.edit { it[KEY_THEME] = mode.api }
    }

    suspend fun setRussianFirst(value: Boolean) {
        context.dataStore.edit { it[KEY_RUSSIAN_FIRST] = value }
    }

    val isSignedIn: Boolean get() = !cachedToken.isNullOrBlank()

    private companion object {
        val KEY_TOKEN = stringPreferencesKey("token")
        val KEY_LANGUAGE = stringPreferencesKey("language")
        val KEY_THEME = stringPreferencesKey("theme")
        val KEY_RUSSIAN_FIRST = booleanPreferencesKey("russian_first")
    }
}
