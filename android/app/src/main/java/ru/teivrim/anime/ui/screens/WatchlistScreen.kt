package ru.teivrim.anime.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.outlined.StarBorder
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import ru.teivrim.anime.data.WatchStatus
import ru.teivrim.anime.ui.CatalogStatus
import ru.teivrim.anime.ui.ErrorKind
import ru.teivrim.anime.ui.ListViewModel
import ru.teivrim.anime.ui.Strings
import ru.teivrim.anime.ui.WatchFilter
import ru.teivrim.anime.ui.components.AnimeCard
import ru.teivrim.anime.ui.components.MessageState
import ru.teivrim.anime.ui.components.TagChip

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun WatchlistScreen(
    vm: ListViewModel,
    strings: Strings,
    russianFirst: Boolean,
    coverUrl: (String?) -> String?,
    onBack: () -> Unit,
    onOpen: (String) -> Unit,
    onSignInRequired: () -> Unit,
) {
    val entries by vm.entries.collectAsStateWithLifecycle()
    val counts by vm.counts.collectAsStateWithLifecycle()
    val status by vm.status.collectAsStateWithLifecycle()
    val filter by vm.filter.collectAsStateWithLifecycle()

    LaunchedEffect(Unit) { vm.load() }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(strings.myList) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, strings.back)
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            val tabs = buildList<Pair<WatchFilter, String>> {
                add(WatchFilter.All to strings.myList)
                add(WatchFilter.Favorites to strings.favourites)
                add(WatchFilter.Status(WatchStatus.Watching) to strings.watchStatusLabel("watching"))
                add(WatchFilter.Status(WatchStatus.Planned) to strings.watchStatusLabel("planned"))
                add(WatchFilter.Status(WatchStatus.Completed) to strings.watchStatusLabel("completed"))
                add(WatchFilter.Status(WatchStatus.Dropped) to strings.watchStatusLabel("dropped"))
            }

            LazyRow(
                Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                items(tabs, key = { it.second }) { (value, label) ->
                    TagChip(
                        text = label,
                        accent = filter == value,
                        onClick = { vm.setFilter(value) },
                    )
                }
            }

            Box(Modifier.weight(1f)) {
                when {
                    status is CatalogStatus.Loading && entries.isEmpty() ->
                        CircularProgressIndicator(Modifier.align(Alignment.Center))

                    status is CatalogStatus.Failed && entries.isEmpty() -> {
                        val kind = (status as? CatalogStatus.Failed)?.kind
                        MessageState(
                            title = if (kind == ErrorKind.Auth) strings.signInRequired else strings.error,
                            hint = if (kind == ErrorKind.Auth) null else strings.errorHint,
                            actionLabel = if (kind == ErrorKind.Auth) strings.signIn else strings.retry,
                            onAction = {
                                if (kind == ErrorKind.Auth) onSignInRequired() else vm.load()
                            },
                            modifier = Modifier.align(Alignment.Center),
                        )
                    }

                    entries.isEmpty() -> MessageState(
                        title = strings.listEmpty,
                        hint = strings.listEmptyHint,
                        modifier = Modifier.align(Alignment.Center),
                    )

                    else -> LazyVerticalGrid(
                        columns = GridCells.Adaptive(minSize = 148.dp),
                        contentPadding = PaddingValues(16.dp),
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                        verticalArrangement = Arrangement.spacedBy(12.dp),
                        modifier = Modifier.fillMaxSize(),
                    ) {
                        items(entries, key = { it.uid }) { entry ->
                            AnimeCard(
                                summary = entry,
                                russianFirst = russianFirst,
                                isFavorite = entry.library.isFavorite,
                                coverUrl = coverUrl(entry.cover),
                                onClick = { onOpen(entry.uid) },
                                onToggleFavorite = { vm.toggleFavorite(entry) },
                            )
                        }
                    }
                }
            }
        }
    }
}
