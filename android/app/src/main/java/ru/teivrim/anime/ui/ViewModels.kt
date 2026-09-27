package ru.teivrim.anime.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.launch
import ru.teivrim.anime.data.AnimeDetail
import ru.teivrim.anime.data.AnimeRepository
import ru.teivrim.anime.data.AnimeSummary
import ru.teivrim.anime.data.ApiException
import ru.teivrim.anime.data.CatalogFilter
import ru.teivrim.anime.data.Filters
import ru.teivrim.anime.data.Genre
import ru.teivrim.anime.data.Suggestion
import ru.teivrim.anime.data.WatchStatus

/** What the catalogue screen is showing right now. */
sealed interface CatalogStatus {
    data object Loading : CatalogStatus
    data object Ready : CatalogStatus
    data class Failed(val kind: ErrorKind) : CatalogStatus
}

enum class ErrorKind { Network, RateLimited, Server, NotFound, Auth }

/** `debounce` is still preview API; the whole ViewModel opts in once. */
@OptIn(FlowPreview::class)
class CatalogViewModel(private val repo: AnimeRepository) : ViewModel() {

    /** Whether there is a session to write to. Screens ask before offering a
     *  list action rather than sending a request that comes back 401. */
    fun signedIn(): Boolean = repo.signedIn


    private val _filter = MutableStateFlow(CatalogFilter())
    val filter: StateFlow<CatalogFilter> = _filter.asStateFlow()

    private val _items = MutableStateFlow<List<AnimeSummary>>(emptyList())
    val items: StateFlow<List<AnimeSummary>> = _items.asStateFlow()

    private val _status = MutableStateFlow<CatalogStatus>(CatalogStatus.Loading)
    val status: StateFlow<CatalogStatus> = _status.asStateFlow()

    private val _total = MutableStateFlow(0L)
    val total: StateFlow<Long> = _total.asStateFlow()

    private val _filters = MutableStateFlow<Filters?>(null)
    val filters: StateFlow<Filters?> = _filters.asStateFlow()

    private val _genres = MutableStateFlow<List<Genre>>(emptyList())
    val genres: StateFlow<List<Genre>> = _genres.asStateFlow()

    private val _suggestions = MutableStateFlow<List<Suggestion>>(emptyList())
    val suggestions: StateFlow<List<Suggestion>> = _suggestions.asStateFlow()

    private val _favUids = MutableStateFlow<Set<String>>(emptySet())
    val favUids: StateFlow<Set<String>> = _favUids.asStateFlow()

    private var page = 1
    private var hasMore = true
    private var loadJob: Job? = null

    init {
        viewModelScope.launch {
            _filter
                // `drop(1)` so the initial empty filter does not fire a request
                // before the ViewModel's own first load.
                .drop(1)
                .debounce(300)
                .distinctUntilChanged()
                .collect { reload() }
        }
        refresh()
    }

    fun refresh() {
        loadFacets()
        loadFavorites()
        reload()
    }

    private fun reload() {
        page = 1
        hasMore = true
        loadJob?.cancel()
        loadJob = viewModelScope.launch { fetchPage(replace = true) }
    }

    fun loadMore() {
        if (!hasMore || _status.value is CatalogStatus.Loading) return
        loadJob = viewModelScope.launch { fetchPage(replace = false) }
    }

    private suspend fun fetchPage(replace: Boolean) {
        if (replace) {
            _status.value = CatalogStatus.Loading
        }
        try {
            val result = repo.list(_filter.value, page)
            _items.value = if (replace) result.items else _items.value + result.items
            _total.value = result.total
            hasMore = result.hasMore
            page += 1
            _status.value = CatalogStatus.Ready
        } catch (e: ApiException) {
            // A failed "load more" keeps what is already on screen: the user
            // still has a usable list, and retrying is a scroll away.
            if (replace) _status.value = CatalogStatus.Failed(e.toKind())
        }
    }

    private fun loadFacets() {
        if (_filters.value != null) return
        viewModelScope.launch {
            runCatching { repo.filters() }.onSuccess { _filters.value = it }
            runCatching { repo.genres() }.onSuccess { _genres.value = it }
        }
    }

    private fun loadFavorites() {
        if (!repo.signedIn) {
            _favUids.value = emptySet()
            return
        }
        viewModelScope.launch {
            runCatching { repo.watchlist(limit = 500) }
                .onSuccess { list -> _favUids.value = list.map { it.uid }.toSet() }
                .onFailure { if (it is ApiException.Unauthorized) _favUids.value = emptySet() }
        }
    }

    fun onQueryChanged(term: String) {
        _filter.value = _filter.value.copy(q = term)
    }

    /** Suggestions are requested on every keystroke; the job is cancelled so a
     *  slow response cannot overwrite a newer one. */
    private var suggestJob: Job? = null

    fun onSearchInput(term: String) {
        suggestJob?.cancel()
        if (term.trim().length < 2) {
            _suggestions.value = emptyList()
            return
        }
        suggestJob = viewModelScope.launch {
            runCatching { repo.suggest(term.trim()) }
                .onSuccess { _suggestions.value = it }
        }
    }

    fun apply(next: CatalogFilter) {
        _filter.value = next
    }

    /** Clears one filter, addressed by the key used in `CatalogFilter.toQuery`. */
    fun removeFilter(key: String) {
        _filter.value = _filter.value.without(key)
    }

    fun clearFilters() {
        _filter.value = CatalogFilter()
    }

    fun dismissSuggestions() {
        _suggestions.value = emptyList()
    }

    fun toggleFavorite(uid: String) {
        val wasFav = uid in _favUids.value
        // Optimistic: the star has to respond immediately or the list feels
        // broken on a slow connection.
        _favUids.value = if (wasFav) _favUids.value - uid else _favUids.value + uid
        viewModelScope.launch {
            runCatching { repo.favorite(uid, !wasFav) }
                .onFailure { _favUids.value = if (wasFav) _favUids.value + uid else _favUids.value - uid }
        }
    }
}

fun ApiException.toKind(): ErrorKind = when (this) {
    is ApiException.Network -> ErrorKind.Network
    is ApiException.RateLimited -> ErrorKind.RateLimited
    is ApiException.Unauthorized -> ErrorKind.Auth
    is ApiException.Client -> if (code == 404) ErrorKind.NotFound else ErrorKind.Server
    is ApiException.Server -> ErrorKind.Server
}

class DetailViewModel(private val repo: AnimeRepository) : ViewModel() {

    fun signedIn(): Boolean = repo.signedIn

    private val _state = MutableStateFlow<DetailState>(DetailState.Loading)
    val state: StateFlow<DetailState> = _state.asStateFlow()

    fun load(uid: String) {
        if (uid.isBlank()) {
            _state.value = DetailState.Failed(ErrorKind.NotFound)
            return
        }
        _state.value = DetailState.Loading
        viewModelScope.launch {
            try {
                val detail = repo.detail(uid)
                _state.value = DetailState.Ready(detail)
            } catch (e: ApiException) {
                _state.value = DetailState.Failed(e.toKind())
            }
        }
    }

    fun save(payload: ru.teivrim.anime.data.UpsertPayload) {
        val current = (_state.value as? DetailState.Ready)?.detail ?: return
        viewModelScope.launch {
            runCatching { repo.saveEntry(payload) }
                .onSuccess { entry ->
                    _state.value = DetailState.Ready(current.copy(library = entry))
                }
        }
    }

    fun remove() {
        val current = (_state.value as? DetailState.Ready)?.detail ?: return
        viewModelScope.launch {
            runCatching { repo.removeFromList(current.uid) }
                .onSuccess { _state.value = DetailState.Ready(current.copy(library = null)) }
        }
    }
}

sealed interface DetailState {
    data object Loading : DetailState
    data class Ready(val detail: AnimeDetail) : DetailState
    data class Failed(val kind: ErrorKind) : DetailState
}

class ListViewModel(private val repo: AnimeRepository) : ViewModel() {

    private val _entries = MutableStateFlow<List<ru.teivrim.anime.data.ListEntry>>(emptyList())
    val entries: StateFlow<List<ru.teivrim.anime.data.ListEntry>> = _entries.asStateFlow()

    private val _counts = MutableStateFlow<ru.teivrim.anime.data.FavoritesCounts?>(null)
    val counts: StateFlow<ru.teivrim.anime.data.FavoritesCounts?> = _counts.asStateFlow()

    private val _status = MutableStateFlow<CatalogStatus>(CatalogStatus.Loading)
    val status: StateFlow<CatalogStatus> = _status.asStateFlow()

    private val _filter = MutableStateFlow<WatchFilter>(WatchFilter.All)
    val filter: StateFlow<WatchFilter> = _filter.asStateFlow()

    fun load() {
        if (!repo.signedIn) {
            _status.value = CatalogStatus.Failed(ErrorKind.Auth)
            return
        }
        _status.value = CatalogStatus.Loading
        viewModelScope.launch {
            try {
                val sel = _filter.value
                _entries.value = repo.watchlist(
                    status = (sel as? WatchFilter.Status)?.status,
                    favoritesOnly = sel is WatchFilter.Favorites,
                )
                _counts.value = repo.counts()
                _status.value = CatalogStatus.Ready
            } catch (e: ApiException) {
                _status.value = CatalogStatus.Failed(e.toKind())
            }
        }
    }

    fun setFilter(value: WatchFilter) {
        _filter.value = value
        load()
    }

    /**
     * Stars an entry without refetching the list.
     *
     * The star is the fastest action on this screen, so it applies optimistically
     * and only reverts if the write fails. A full reload would also throw away
     * the scroll position.
     */
    fun toggleFavorite(entry: ru.teivrim.anime.data.ListEntry) {
        val target = !entry.library.isFavorite
        fun apply(value: Boolean) = _entries.value.map {
            if (it.uid == entry.uid) it.copy(library = it.library.copy(isFavorite = value)) else it
        }
        _entries.value = apply(target)
        viewModelScope.launch {
            runCatching { repo.favorite(entry.uid, target) }
                .onFailure { _entries.value = apply(!target) }
        }
    }
}

sealed interface WatchFilter {
    data object All : WatchFilter
    data object Favorites : WatchFilter
    data class Status(val status: WatchStatus) : WatchFilter
}

class AuthViewModel(private val repo: AnimeRepository) : ViewModel() {

    private val _state = MutableStateFlow<AuthState>(AuthState.Idle)
    val state: StateFlow<AuthState> = _state.asStateFlow()

    fun submit(
        signUp: Boolean,
        username: String,
        email: String,
        password: String,
        confirm: String,
        onSuccess: () -> Unit,
    ) {
        if (signUp) {
            if (username.trim().length < 3) {
                _state.value = AuthState.Invalid(AuthError.ShortUsername)
                return
            }
            if (password != confirm) {
                _state.value = AuthState.Invalid(AuthError.PasswordMismatch)
                return
            }
        }
        if (password.isEmpty()) {
            _state.value = AuthState.Invalid(AuthError.EmptyPassword)
            return
        }

        _state.value = AuthState.Busy
        viewModelScope.launch {
            try {
                if (signUp) {
                    repo.register(username.trim(), email.trim(), password)
                } else {
                    repo.login(username.trim(), password)
                }
                _state.value = AuthState.Done
                onSuccess()
            } catch (e: ApiException) {
                _state.value = AuthState.Failed(e.message.orEmpty())
            }
        }
    }

    fun reset() {
        _state.value = AuthState.Idle
    }
}

sealed interface AuthState {
    data object Idle : AuthState
    data object Busy : AuthState
    data object Done : AuthState
    data class Invalid(val error: AuthError) : AuthState
    data class Failed(val message: String) : AuthState
}

enum class AuthError { ShortUsername, PasswordMismatch, EmptyPassword }
