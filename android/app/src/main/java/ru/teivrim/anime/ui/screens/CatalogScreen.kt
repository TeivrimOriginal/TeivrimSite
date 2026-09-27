package ru.teivrim.anime.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import coil.compose.AsyncImage
import ru.teivrim.anime.data.CatalogFilter
import ru.teivrim.anime.ui.CatalogStatus
import ru.teivrim.anime.ui.CatalogViewModel
import ru.teivrim.anime.ui.ErrorKind
import ru.teivrim.anime.ui.Strings
import ru.teivrim.anime.ui.components.AnimeCard
import ru.teivrim.anime.ui.components.MessageState
import ru.teivrim.anime.ui.components.PillButton
import ru.teivrim.anime.ui.components.SkeletonGrid
import ru.teivrim.anime.ui.components.TagChip
import ru.teivrim.anime.ui.theme.Dimens

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CatalogScreen(
    vm: CatalogViewModel,
    strings: Strings,
    russianFirst: Boolean,
    coverUrl: (String?) -> String?,
    onOpen: (String) -> Unit,
    onSignInRequired: () -> Unit,
) {
    val filter by vm.filter.collectAsStateWithLifecycle()
    val items by vm.items.collectAsStateWithLifecycle()
    val status by vm.status.collectAsStateWithLifecycle()
    val total by vm.total.collectAsStateWithLifecycle()
    val suggestions by vm.suggestions.collectAsStateWithLifecycle()
    val favUids by vm.favUids.collectAsStateWithLifecycle()
    val genres by vm.genres.collectAsStateWithLifecycle()
    val filters by vm.filters.collectAsStateWithLifecycle()

    val gridState = rememberLazyGridState()
    var sheetOpen by remember { mutableStateOf(false) }
    var draft by remember(filter) { mutableStateOf(filter) }
    var searchText by remember(filter.q) { mutableStateOf(filter.q) }

    // Prefetch the next page two rows before the end, so scrolling never stalls.
    val shouldLoadMore by remember {
        derivedStateOf {
            val last = gridState.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0
            val totalItems = gridState.layoutInfo.totalItemsCount
            totalItems > 0 && last >= totalItems - 4
        }
    }
    LaunchedEffect(shouldLoadMore) {
        if (shouldLoadMore) vm.loadMore()
    }

    Scaffold(
        topBar = {
            Column {
                Surface(color = MaterialTheme.colorScheme.surfaceContainer) {
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .padding(horizontal = Dimens.screenPadding, vertical = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        OutlinedTextField(
                            value = searchText,
                            onValueChange = {
                                searchText = it
                                vm.onSearchInput(it)
                            },
                            placeholder = { Text(strings.searchHint) },
                            singleLine = true,
                            leadingIcon = { Icon(Icons.Filled.Search, null) },
                            trailingIcon = {
                                if (searchText.isNotEmpty()) {
                                    IconButton(onClick = {
                                        searchText = ""
                                        vm.onQueryChanged("")
                                        vm.dismissSuggestions()
                                    }) {
                                        Icon(Icons.Filled.Close, strings.cancel)
                                    }
                                }
                            },
                            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                            keyboardActions = KeyboardActions(onSearch = {
                                vm.onQueryChanged(searchText)
                                vm.dismissSuggestions()
                            }),
                            shape = RoundedCornerShape(12.dp),
                            modifier = Modifier
                                .weight(1f)
                                .heightIn(min = 52.dp),
                        )
                        Spacer(Modifier.width(8.dp))
                        PillButton(
                            text = strings.filters,
                            onClick = {
                                draft = filter
                                sheetOpen = true
                            },
                            primary = filter.activeCount() > 0,
                        )
                    }
                }

                if (suggestions.isNotEmpty()) {
                    Surface(
                        color = MaterialTheme.colorScheme.surfaceContainer,
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        LazyRow(
                            Modifier.fillMaxWidth().padding(vertical = 4.dp),
                            contentPadding = PaddingValues(horizontal = Dimens.screenPadding),
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            items(suggestions, key = { it.uid }) { s ->
                                SuggestionRow(
                                    title = s.title,
                                    subtitle = listOfNotNull(
                                        s.titleRomaji, s.titleEnglish, s.titleRussian,
                                    ).firstOrNull { it != s.title },
                                    cover = coverUrl(s.cover),
                                    onClick = {
                                        vm.dismissSuggestions()
                                        onOpen(s.uid)
                                    },
                                )
                            }
                        }
                    }
                }
            }
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            FilterChips(
                filter = filter,
                strings = strings,
                genres = genres,
                onRemove = { key ->
                    // The search box is the query's editor, so it has to follow
                    // when the query is the chip that was dismissed.
                    if (key == "q") {
                        searchText = ""
                        vm.dismissSuggestions()
                    }
                    vm.removeFilter(key)
                },
            )

            Box(Modifier.weight(1f)) {
                // Bound before the `when` because a delegated property cannot be
                // smart-cast, and a `when` subject cannot carry `&&` guards.
                val failure = status as? CatalogStatus.Failed
                when {
                    status is CatalogStatus.Loading && items.isEmpty() -> {
                        Column(Modifier.padding(Dimens.screenPadding)) {
                            Spacer(Modifier.height(60.dp))
                            SkeletonGrid(columns = 3)
                        }
                    }

                    failure != null && items.isEmpty() -> {
                        MessageState(
                            title = strings.error,
                            hint = when (failure.kind) {
                                ErrorKind.Network -> strings.offline
                                ErrorKind.RateLimited -> strings.rateLimited
                                ErrorKind.Auth -> strings.signInRequired
                                else -> strings.errorHint
                            },
                            actionLabel = if (failure.kind == ErrorKind.Auth) strings.signIn else strings.retry,
                            onAction = {
                                if (failure.kind == ErrorKind.Auth) onSignInRequired() else vm.refresh()
                            },
                            modifier = Modifier.align(Alignment.Center),
                        )
                    }

                    items.isEmpty() -> {
                        MessageState(
                            title = strings.nothing,
                            hint = strings.nothingHint,
                            actionLabel = strings.reset,
                            onAction = {
                                searchText = ""
                                vm.clearFilters()
                            },
                            modifier = Modifier.align(Alignment.Center),
                        )
                    }

                    else -> {
                        LazyVerticalGrid(
                            columns = GridCells.Adaptive(minSize = Dimens.cardMinWidth),
                            state = gridState,
                            contentPadding = PaddingValues(Dimens.screenPadding),
                            horizontalArrangement = Arrangement.spacedBy(Dimens.gridGap),
                            verticalArrangement = Arrangement.spacedBy(Dimens.gridGap),
                            modifier = Modifier.fillMaxSize(),
                        ) {
                            items(items, key = { it.uid }) { summary ->
                                AnimeCard(
                                    summary = summary,
                                    russianFirst = russianFirst,
                                    isFavorite = summary.uid in favUids,
                                    coverUrl = coverUrl(summary.cover),
                                    onClick = { onOpen(summary.uid) },
                                    onToggleFavorite = {
                                        if (vm.signedIn()) vm.toggleFavorite(summary.uid) else onSignInRequired()
                                    },
                                )
                            }
                            item(span = { GridItemSpan(maxLineSpan) }) {
                                Row(
                                    Modifier
                                        .fillMaxWidth()
                                        .padding(vertical = 16.dp),
                                    horizontalArrangement = Arrangement.Center,
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    if (status is CatalogStatus.Loading) {
                                        CircularProgressIndicator(
                                            Modifier.size(22.dp),
                                            strokeWidth = 2.dp,
                                        )
                                        Spacer(Modifier.width(10.dp))
                                    }
                                    Text(
                                        text = "${strings.found}: ${"%,d".format(total)}",
                                        style = MaterialTheme.typography.labelMedium,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if (sheetOpen) {
        val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
        ModalBottomSheet(
            onDismissRequest = { sheetOpen = false },
            sheetState = sheetState,
        ) {
            FilterSheet(
                draft = draft,
                strings = strings,
                filters = filters,
                genres = genres,
                onChange = { draft = it },
                onReset = { draft = CatalogFilter() },
                onApply = {
                    vm.apply(draft)
                    sheetOpen = false
                },
            )
        }
    }
}

@Composable
private fun SuggestionRow(
    title: String,
    subtitle: String?,
    cover: String?,
    onClick: () -> Unit,
) {
    Row(
        Modifier
            .width(230.dp)
            .clip(RoundedCornerShape(10.dp))
            .clickable(onClick = onClick)
            .padding(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        AsyncImage(
            model = cover,
            contentDescription = null,
            contentScale = ContentScale.Crop,
            modifier = Modifier
                .size(width = 32.dp, height = 48.dp)
                .clip(RoundedCornerShape(5.dp))
                .background(MaterialTheme.colorScheme.surfaceContainerHighest),
        )
        Spacer(Modifier.width(8.dp))
        Column {
            Text(
                title,
                style = MaterialTheme.typography.bodyMedium,
                fontWeight = FontWeight.SemiBold,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (!subtitle.isNullOrBlank()) {
                Text(
                    subtitle,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

/** Horizontal row of the active filters, each removable. */
@Composable
private fun FilterChips(
    filter: CatalogFilter,
    strings: Strings,
    genres: List<ru.teivrim.anime.data.Genre>,
    onRemove: (String) -> Unit,
) {
    val entries = buildList {
        if (filter.q.isNotBlank()) add("q" to filter.q)
        if (filter.sort != "popularity") {
            add("sort" to strings.sorts().firstOrNull { it.first == filter.sort }?.second.orEmpty())
        }
        if (filter.genre.isNotBlank()) {
            add("genre" to (genres.firstOrNull { it.slug == filter.genre }?.display() ?: filter.genre))
        }
        if (filter.format.isNotBlank()) add("format" to filter.format)
        if (filter.status.isNotBlank()) add("status" to strings.statusLabel(filter.status))
        if (filter.season.isNotBlank()) add("season" to strings.seasonLabel(filter.season))
        if (filter.country.isNotBlank()) add("country" to filter.country)
        if (filter.adult == "no") add("adult" to strings.adultSafe)
        if (filter.adult == "only") add("adult" to strings.adultOnly)
        if (filter.hasRussian == "yes") add("has_ru" to strings.hasRussianOnly)
        if (filter.hasTrailer == "yes") add("has_trailer" to strings.hasTrailer)
        if (filter.yearFrom.isNotBlank() || filter.yearTo.isNotBlank()) {
            add("year" to "${filter.yearFrom.ifBlank { "…" }}–${filter.yearTo.ifBlank { "…" }}")
        }
        if (filter.scoreFrom.isNotBlank() || filter.scoreTo.isNotBlank()) {
            add("score" to "${filter.scoreFrom.ifBlank { "…" }}–${filter.scoreTo.ifBlank { "…" }}")
        }
    }
    if (entries.isEmpty()) return

    LazyRow(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = Dimens.screenPadding, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        items(entries, key = { it.first }) { (key, label) ->
            TagChip(
                text = "$label  ×",
                accent = true,
                onClick = { onRemove(key) },
            )
        }
    }
}
