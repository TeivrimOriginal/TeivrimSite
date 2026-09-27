package ru.teivrim.anime.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.outlined.StarBorder
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import coil.compose.AsyncImage
import ru.teivrim.anime.data.AnimeDetail
import ru.teivrim.anime.data.LibraryEntry
import ru.teivrim.anime.data.Person
import ru.teivrim.anime.data.UpsertPayload
import ru.teivrim.anime.data.WatchStatus
import ru.teivrim.anime.ui.DetailState
import ru.teivrim.anime.ui.DetailViewModel
import ru.teivrim.anime.ui.ErrorKind
import ru.teivrim.anime.ui.Strings
import ru.teivrim.anime.ui.components.MessageState
import ru.teivrim.anime.ui.components.PillButton
import ru.teivrim.anime.ui.components.SectionTitle
import ru.teivrim.anime.ui.components.TagChip

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DetailScreen(
    uid: String,
    vm: DetailViewModel,
    strings: Strings,
    russianFirst: Boolean,
    coverUrl: (String?) -> String?,
    onBack: () -> Unit,
    onOpen: (String) -> Unit,
    onOpenUrl: (String) -> Unit,
    onSignInRequired: () -> Unit,
) {
    val state by vm.state.collectAsStateWithLifecycle()
    var editorOpen by remember { mutableStateOf(false) }

    LaunchedEffect(uid) { vm.load(uid) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        text = (state as? DetailState.Ready)?.detail?.title(russianFirst).orEmpty(),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, strings.back)
                    }
                },
            )
        },
    ) { padding ->
        Box(Modifier.padding(padding).fillMaxSize()) {
            when (val s = state) {
                is DetailState.Loading -> CircularProgressIndicator(Modifier.align(Alignment.Center))

                is DetailState.Failed -> MessageState(
                    title = if (s.kind == ErrorKind.NotFound) strings.notFound else strings.error,
                    hint = if (s.kind == ErrorKind.NotFound) strings.notFoundHint else strings.errorHint,
                    actionLabel = if (s.kind == ErrorKind.NotFound) strings.back else strings.retry,
                    onAction = { if (s.kind == ErrorKind.NotFound) onBack() else vm.load(uid) },
                    modifier = Modifier.align(Alignment.Center),
                )

                is DetailState.Ready -> DetailBody(
                    detail = s.detail,
                    strings = strings,
                    russianFirst = russianFirst,
                    coverUrl = coverUrl,
                    onOpen = onOpen,
                    onOpenUrl = onOpenUrl,
                    onEditList = {
                        if (vm.signedIn()) editorOpen = true else onSignInRequired()
                    },
                )
            }
        }
    }

    if (editorOpen) {
        val detail = (state as? DetailState.Ready)?.detail
        if (detail != null) {
            LibraryEditorDialog(
                detail = detail,
                strings = strings,
                onDismiss = { editorOpen = false },
                onSave = { payload ->
                    vm.save(payload)
                    editorOpen = false
                },
                onRemove = {
                    vm.remove()
                    editorOpen = false
                },
            )
        }
    }
}

@Composable
private fun DetailBody(
    detail: AnimeDetail,
    strings: Strings,
    russianFirst: Boolean,
    coverUrl: (String?) -> String?,
    onOpen: (String) -> Unit,
    onOpenUrl: (String) -> Unit,
    onEditList: () -> Unit,
) {
    val title = detail.title(russianFirst)
    val inList = detail.library != null

    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(bottom = 40.dp),
    ) {
        item {
            Box(Modifier.fillMaxWidth().height(230.dp)) {
                detail.banner?.let { banner ->
                    AsyncImage(
                        model = coverUrl(banner),
                        contentDescription = null,
                        contentScale = ContentScale.Crop,
                        modifier = Modifier.fillMaxSize(),
                    )
                    // Fade into the page background so the poster below reads as
                    // sitting on top rather than pasted on.
                    Box(
                        Modifier
                            .fillMaxSize()
                            .background(
                                Brush.verticalGradient(
                                    0f to Color.Transparent,
                                    0.6f to MaterialTheme.colorScheme.background.copy(alpha = 0.6f),
                                    1f to MaterialTheme.colorScheme.background,
                                )
                            )
                    )
                }
            }
        }

        item {
            Row(
                Modifier.padding(horizontal = 20.dp),
                verticalAlignment = Alignment.Bottom,
            ) {
                AsyncImage(
                    model = coverUrl(detail.cover),
                    contentDescription = null,
                    contentScale = ContentScale.Crop,
                    modifier = Modifier
                        .width(150.dp)
                        .aspectRatio(2f / 3f)
                        .clip(RoundedCornerShape(12.dp))
                        .background(MaterialTheme.colorScheme.surfaceContainerHighest),
                )
                Spacer(Modifier.width(16.dp))
                Column(Modifier.weight(1f)) {
                    Text(
                        text = title,
                        style = MaterialTheme.typography.titleLarge,
                        fontWeight = FontWeight.ExtraBold,
                    )
                    AlternateTitles(detail, title, strings)
                }
            }
        }

        item {
            PillRow(detail, strings)
        }

        item {
            Row(
                Modifier.padding(horizontal = 20.dp, vertical = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Button(
                    onClick = onEditList,
                    modifier = Modifier.weight(1f),
                ) {
                    Icon(
                        imageVector = if (inList) Icons.Filled.Star else Icons.Outlined.StarBorder,
                        contentDescription = null,
                        modifier = Modifier.size(18.dp),
                    )
                    Spacer(Modifier.width(8.dp))
                    // Both states open the same editor; removal lives inside it,
                    // so the label stays honest about what the tap does.
                    Text(if (inList) strings.editList else strings.addToList)
                }
                detail.trailer?.let { trailer ->
                    OutlinedButton(
                        onClick = { onOpenUrl(trailer.url) },
                        modifier = Modifier.weight(1f),
                    ) {
                        Text(strings.trailer)
                    }
                }
            }
        }

        if (detail.library != null) {
            item { LibrarySummary(detail.library, strings) }
        }

        // Russian first when it exists, because that is what a Russian reader
        // wants; the AniList text follows rather than replacing it.
        if (russianFirst && !detail.descriptionRu.isNullOrBlank()) {
            item {
                SectionBlock(strings.descriptionRu) {
                    Text(detail.descriptionRu, style = MaterialTheme.typography.bodyLarge)
                }
            }
        }
        if (!detail.description.isNullOrBlank()) {
            item {
                SectionBlock(
                    if (russianFirst) strings.descriptionEn else strings.description
                ) {
                    Text(detail.description, style = MaterialTheme.typography.bodyLarge)
                }
            }
        }
        if (!russianFirst && !detail.descriptionRu.isNullOrBlank()) {
            item {
                SectionBlock(strings.descriptionRu) {
                    Text(detail.descriptionRu, style = MaterialTheme.typography.bodyLarge)
                }
            }
        }

        if (detail.genres.isNotEmpty()) {
            item {
                SectionBlock(strings.genres) {
                    TagFlow(detail.genres.map { it.display() }, accent = true)
                }
            }
        }
        if (detail.studios.isNotEmpty()) {
            item { SectionBlock(strings.studios) { TagFlow(detail.studios.map { it.display() }) } }
        }
        if (detail.producers.isNotEmpty()) {
            item { SectionBlock(strings.producers) { TagFlow(detail.producers.map { it.display() }) } }
        }
        if (detail.licensors.isNotEmpty()) {
            item { SectionBlock(strings.licensors) { TagFlow(detail.licensors.map { it.display() }) } }
        }
        if (detail.synonyms.isNotEmpty()) {
            item { SectionBlock(strings.synonyms) { TagFlow(detail.synonyms) } }
        }
        if (detail.tags.isNotEmpty()) {
            item {
                SectionBlock(strings.tags) {
                    TagFlow(detail.tags.map { tag ->
                        tag.name + (tag.rank?.let { " · $it%" } ?: "")
                    })
                }
            }
        }

        if (detail.characters.isNotEmpty()) {
            item { SectionBlock("${strings.cast} (${detail.characters.size})") { PersonList(detail.characters.take(40), true, strings, coverUrl) } }
        }
        if (detail.staff.isNotEmpty()) {
            item { SectionBlock("${strings.staff} (${detail.staff.size})") { PersonList(detail.staff.take(24), false, strings, coverUrl) } }
        }

        if (detail.recommendations.isNotEmpty()) {
            item {
                SectionBlock(strings.recommendations) {
                    MiniGrid(
                        detail.recommendations.map {
                            Triple(it.uid, it.title, coverUrl(it.cover))
                        },
                        onOpen = onOpen,
                    )
                }
            }
        }
        if (detail.relations.isNotEmpty()) {
            item {
                SectionBlock(strings.related) {
                    MiniGrid(
                        detail.relations.map {
                            Triple(it.uid, it.title, coverUrl(it.cover))
                        },
                        onOpen = onOpen,
                    )
                }
            }
        }

        item { SectionBlock(strings.information) { InfoTable(detail, strings) } }

        if (detail.externalLinks.isNotEmpty()) {
            item {
                SectionBlock(strings.externalLinks) {
                    LinkFlow(
                        detail.externalLinks.map {
                            (it.site + (it.type?.let { t -> " · $t" } ?: "")) to it.url
                        },
                        onOpenUrl,
                    )
                }
            }
        }
        if (detail.streaming.isNotEmpty()) {
            item {
                SectionBlock(strings.streaming) {
                    LinkFlow(
                        detail.streaming.take(40).map {
                            (it.site + (it.title?.let { t -> " · $t" } ?: "")) to it.url
                        },
                        onOpenUrl,
                    )
                }
            }
        }
    }
}

@Composable
private fun AlternateTitles(detail: AnimeDetail, main: String, strings: Strings) {
    val pairs = listOf(
        "RU" to detail.titleRussian,
        "EN" to detail.titleEnglish,
        "JP" to detail.titleNative,
        "Romaji" to detail.titleRomaji,
    )
    // The title already shown is skipped, otherwise it appears twice on screen.
    val seen = mutableSetOf(main)
    pairs.forEach { (label, value) ->
        if (!value.isNullOrBlank() && seen.add(value)) {
            Text(
                text = "$label: $value",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
private fun PillRow(detail: AnimeDetail, strings: Strings) {
    // `null` is "no accent"; naming the type keeps the null entries from
    // pinning the list to Pair<String, Nothing?>.
    val pills = buildList<Pair<String, String?>> {
        detail.format?.let { add(it to null) }
        detail.status?.let { add(strings.statusLabel(it) to null) }
        detail.episodes?.let { add("$it ${strings.episodes}" to null) }
        detail.duration?.let { add("$it ${strings.perEpisode}" to null) }
        detail.score?.let { add("★ $it" to "score") }
        detail.ratingCount?.let { add("${"%,d".format(it)} ${strings.ratingCount}" to null) }
        detail.season?.let { add(strings.seasonLabel(it) + (detail.seasonYear?.let { y -> " $y" } ?: "") to null) }
        detail.startDate?.let { add(it.replace('-', '.') to null) }
        detail.country?.let { add(it to null) }
        if (detail.isAdult) add(strings.adultBadge to "adult")
    }
    if (pills.isEmpty()) return

    LazyRow(
        Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 6.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        items(pills, key = { it.first + it.second.orEmpty() }) { (label, kind) ->
            TagChip(
                text = label,
                accent = kind == "adult",
            )
        }
    }
}

@Composable
private fun LibrarySummary(entry: LibraryEntry, strings: Strings) {
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 20.dp, vertical = 6.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        TagChip(text = strings.watchStatusLabel(entry.status), accent = true)
        entry.score?.let { TagChip(text = "★ $it") }
        entry.progress?.let {
            TagChip(text = "$it / ${entry.episodes ?: "?"}")
        }
    }
}

@Composable
private fun SectionBlock(title: String, content: @Composable () -> Unit) {
    Column(Modifier.padding(horizontal = 20.dp, vertical = 12.dp)) {
        SectionTitle(title)
        content()
    }
}

@Composable
private fun TagFlow(items: List<String>, accent: Boolean = false) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        items.chunked(3).forEach { row ->
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                row.forEach { TagChip(it, accent = accent, modifier = Modifier.weight(1f)) }
                repeat(3 - row.size) { Spacer(Modifier.weight(1f)) }
            }
        }
    }
}

/**
 * Same wrapping grid as [TagFlow], but each chip opens a URL.
 *
 * External and streaming links are the reason a reader installs the app, so
 * they have to be tappable rather than decorative text.
 */
@Composable
private fun LinkFlow(links: List<Pair<String, String>>, onOpenUrl: (String) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        links.chunked(2).forEach { row ->
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                row.forEach { (label, url) ->
                    TagChip(
                        text = label,
                        accent = true,
                        onClick = { onOpenUrl(url) },
                        modifier = Modifier.weight(1f),
                    )
                }
                repeat(2 - row.size) { Spacer(Modifier.weight(1f)) }
            }
        }
    }
}

@Composable
private fun PersonList(
    people: List<Person>,
    isCast: Boolean,
    strings: Strings,
    coverUrl: (String?) -> String?,
) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        people.forEach { person ->
            Row(
                Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(10.dp))
                    .background(MaterialTheme.colorScheme.surfaceContainer)
                    .padding(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                AsyncImage(
                    model = coverUrl(person.image),
                    contentDescription = null,
                    contentScale = ContentScale.Crop,
                    modifier = Modifier
                        .size(width = 36.dp, height = 50.dp)
                        .clip(RoundedCornerShape(6.dp))
                        .background(MaterialTheme.colorScheme.surfaceContainerHighest),
                )
                Spacer(Modifier.width(10.dp))
                Column(Modifier.weight(1f)) {
                    if (isCast && !person.role.isNullOrBlank()) {
                        Text(
                            person.role,
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.primary,
                        )
                    }
                    Text(
                        person.name,
                        style = MaterialTheme.typography.bodyMedium,
                        fontWeight = FontWeight.SemiBold,
                    )
                    val meta = when {
                        isCast && !person.voiceActor.isNullOrBlank() ->
                            "${strings.voiceActor}: ${person.voiceActor}"
                        !isCast && !person.positions.isNullOrEmpty() -> person.positions.joinToString(", ")
                        else -> null
                    }
                    if (meta != null) {
                        Text(
                            meta,
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun MiniGrid(
    items: List<Triple<String, String, String?>>,
    onOpen: (String) -> Unit,
) {
    LazyRow(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        items(items, key = { it.first }) { (uid, title, cover) ->
            Column(
                Modifier
                    .width(112.dp)
                    .clip(RoundedCornerShape(10.dp))
                    .clickable { onOpen(uid) }
                    .background(MaterialTheme.colorScheme.surfaceContainer),
            ) {
                AsyncImage(
                    model = cover,
                    contentDescription = null,
                    contentScale = ContentScale.Crop,
                    modifier = Modifier
                        .fillMaxWidth()
                        .aspectRatio(2f / 3f)
                        .background(MaterialTheme.colorScheme.surfaceContainerHighest),
                )
                Text(
                    text = title,
                    style = MaterialTheme.typography.labelSmall,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(8.dp),
                )
            }
        }
    }
}

@Composable
private fun InfoTable(detail: AnimeDetail, strings: Strings) {
    val rows = buildList {
        fun row(label: String, value: String?) {
            if (!value.isNullOrBlank()) add(label to value)
        }
        row(strings.format, detail.format)
        row(strings.status, detail.status?.let { strings.statusLabel(it) })
        row(strings.episodes, detail.episodes?.toString())
        row(strings.perEpisode, detail.duration?.toString())
        row(strings.season, detail.season?.let { strings.seasonLabel(it) + (detail.seasonYear?.let { y -> " $y" } ?: "") })
        row(strings.aired, detail.startDate)
        row(strings.ended, detail.endDate)
        row("Страна / Country", detail.country)
        row(strings.score, detail.score?.let { "$it (${detail.scoreSource.orEmpty()})" })
        row(strings.ratingCount, detail.ratingCount?.let { "%,d".format(it) })
        row(strings.popularity, detail.popularity?.let { "%,d".format(it) })
        row(strings.favourites, detail.favourites?.let { "%,d".format(it) })
        row(strings.trending, detail.trending?.let { "%,d".format(it) })
        row(strings.ageRating, detail.ageRating)
        row(
            strings.ids,
            listOfNotNull(
                detail.ids.anilist?.let { "AniList $it" },
                detail.ids.mal?.let { "MAL $it" },
                detail.ids.shikimori?.let { "Shikimori $it" },
                detail.ids.kitsu?.let { "Kitsu $it" },
            ).joinToString(" · "),
        )
    }

    Column {
        rows.forEach { (label, value) ->
            Row(
                Modifier
                    .fillMaxWidth()
                    .padding(vertical = 7.dp),
            ) {
                Text(
                    text = label,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.width(140.dp),
                )
                Text(text = value, style = MaterialTheme.typography.bodyMedium)
            }
        }
    }
}

@Composable
private fun LibraryEditorDialog(
    detail: AnimeDetail,
    strings: Strings,
    onDismiss: () -> Unit,
    onSave: (UpsertPayload) -> Unit,
    onRemove: () -> Unit,
) {
    val current = detail.library
    var status by remember {
        mutableStateOf(current?.status ?: WatchStatus.Planned.api)
    }
    var score by remember { mutableStateOf(current?.score?.toString().orEmpty()) }
    var progress by remember { mutableStateOf(current?.progress?.toString().orEmpty()) }
    var favorite by remember { mutableStateOf(current?.isFavorite ?: false) }
    var notes by remember { mutableStateOf(current?.notes.orEmpty()) }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(strings.myList) },
        text = {
            androidx.compose.foundation.layout.Column(
                Modifier.verticalScroll(rememberScrollState()),
            ) {
                SectionTitle(strings.watchStatus)
                LazyRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    items(WatchStatus.entries.toList(), key = { it.api }) { option ->
                        TagChip(
                            text = strings.watchStatusLabel(option.api),
                            accent = option.api == status,
                            onClick = { status = option.api },
                        )
                    }
                }
                Spacer(Modifier.height(12.dp))
                OutlinedTextField(
                    value = score,
                    onValueChange = { score = it.filter { c -> c.isDigit() }.take(2) },
                    label = { Text("${strings.yourScore} (1–10)") },
                    singleLine = true,
                )
                Spacer(Modifier.height(8.dp))
                OutlinedTextField(
                    value = progress,
                    onValueChange = { progress = it.filter { c -> c.isDigit() } },
                    label = { Text("${strings.yourProgress} / ${detail.episodes ?: "?"}") },
                    singleLine = true,
                )
                Spacer(Modifier.height(8.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(strings.markFavorite, style = MaterialTheme.typography.bodyMedium)
                    Spacer(Modifier.weight(1f))
                    Switch(checked = favorite, onCheckedChange = { favorite = it })
                }
                OutlinedTextField(
                    value = notes,
                    onValueChange = { notes = it },
                    label = { Text(strings.notes) },
                    placeholder = { Text(strings.notesHint) },
                    minLines = 2,
                )
            }
        },
        confirmButton = {
            TextButton(onClick = {
                onSave(
                    UpsertPayload(
                        uid = detail.uid,
                        status = status,
                        isFavorite = favorite,
                        score = score.toIntOrNull(),
                        progress = progress.toIntOrNull(),
                        notes = notes.ifBlank { null },
                    )
                )
            }) {
                Text(strings.save)
            }
        },
        dismissButton = {
            Row {
                if (current != null) {
                    TextButton(onClick = onRemove) { Text(strings.delete, color = MaterialTheme.colorScheme.error) }
                }
                TextButton(onClick = onDismiss) { Text(strings.cancel) }
            }
        },
    )
}
