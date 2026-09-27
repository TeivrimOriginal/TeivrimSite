package ru.teivrim.anime.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import ru.teivrim.anime.data.CatalogFilter
import ru.teivrim.anime.data.Filters
import ru.teivrim.anime.data.Genre
import ru.teivrim.anime.ui.Strings
import ru.teivrim.anime.ui.components.SectionTitle
import ru.teivrim.anime.ui.components.TagChip

/**
 * The filter editor.
 *
 * Edits a local draft and only hands it to the caller on "apply", so dragging
 * through a few options does not fire a request per change.
 */
@Composable
fun FilterSheet(
    draft: CatalogFilter,
    strings: Strings,
    filters: Filters?,
    genres: List<Genre>,
    onChange: (CatalogFilter) -> Unit,
    onReset: () -> Unit,
    onApply: () -> Unit,
) {
    Column(
        Modifier
            .fillMaxWidth()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp)
            .padding(bottom = 28.dp),
    ) {
        SectionTitle(strings.sort)
        DropdownField(
            label = strings.sort,
            options = strings.sorts(),
            selected = draft.sort,
            display = { value -> strings.sorts().firstOrNull { it.first == value }?.second ?: value },
            onSelect = { onChange(draft.copy(sort = it)) },
        )

        Spacer(Modifier.height(14.dp))

        if (genres.isNotEmpty()) {
            SectionTitle(strings.genre)
            GenreGrid(
                genres = genres,
                selected = draft.genre,
                onSelect = { slug ->
                    onChange(draft.copy(genre = if (draft.genre == slug) "" else slug))
                },
            )
            Spacer(Modifier.height(14.dp))
        }

        if (!filters?.formats.isNullOrEmpty()) {
            SectionTitle(strings.format)
            ChipFlow(
                options = filters!!.formats,
                selected = draft.format,
                onSelect = { onChange(draft.copy(format = it)) },
            )
            Spacer(Modifier.height(14.dp))
        }

        if (!filters?.statuses.isNullOrEmpty()) {
            SectionTitle(strings.status)
            ChipFlow(
                options = filters!!.statuses,
                selected = draft.status,
                onSelect = { onChange(draft.copy(status = it)) },
                display = { strings.statusLabel(it) },
            )
            Spacer(Modifier.height(14.dp))
        }

        if (!filters?.countries.isNullOrEmpty()) {
            SectionTitle(strings.country)
            ChipFlow(
                options = filters!!.countries,
                selected = draft.country,
                onSelect = { onChange(draft.copy(country = it)) },
            )
            Spacer(Modifier.height(14.dp))
        }

        if (!filters?.seasons.isNullOrEmpty()) {
            SectionTitle(strings.season)
            ChipFlow(
                options = filters!!.seasons,
                selected = draft.season,
                onSelect = { onChange(draft.copy(season = it)) },
                display = { strings.seasonLabel(it) },
            )
            Spacer(Modifier.height(14.dp))
        }

        SectionTitle(strings.year)
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            NumberField(
                value = draft.yearFrom,
                onValueChange = { onChange(draft.copy(yearFrom = it)) },
                placeholder = strings.yearFrom,
                modifier = Modifier.weight(1f),
            )
            NumberField(
                value = draft.yearTo,
                onValueChange = { onChange(draft.copy(yearTo = it)) },
                placeholder = strings.yearTo,
                modifier = Modifier.weight(1f),
            )
        }
        Spacer(Modifier.height(14.dp))

        SectionTitle(strings.score)
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            NumberField(
                value = draft.scoreFrom,
                onValueChange = { onChange(draft.copy(scoreFrom = it)) },
                placeholder = strings.scoreFrom,
                modifier = Modifier.weight(1f),
            )
            NumberField(
                value = draft.scoreTo,
                onValueChange = { onChange(draft.copy(scoreTo = it)) },
                placeholder = strings.scoreTo,
                modifier = Modifier.weight(1f),
            )
        }
        Spacer(Modifier.height(14.dp))

        SectionTitle(strings.adult)
        ChipFlow(
            options = listOf("", "no", "only"),
            selected = draft.adult,
            onSelect = { onChange(draft.copy(adult = it)) },
            display = { value ->
                when (value) {
                    "no" -> strings.adultSafe
                    "only" -> strings.adultOnly
                    else -> strings.adultAny
                }
            },
        )
        Spacer(Modifier.height(8.dp))

        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = 48.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(strings.hasRussian, style = MaterialTheme.typography.bodyMedium)
            Spacer(Modifier.weight(1f))
            Switch(
                checked = draft.hasRussian == "yes",
                onCheckedChange = { onChange(draft.copy(hasRussian = if (it) "yes" else "")) },
            )
        }

        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = 48.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(strings.hasTrailer, style = MaterialTheme.typography.bodyMedium)
            Spacer(Modifier.weight(1f))
            Switch(
                checked = draft.hasTrailer == "yes",
                onCheckedChange = { onChange(draft.copy(hasTrailer = if (it) "yes" else "")) },
            )
        }

        Spacer(Modifier.height(20.dp))

        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            OutlinedButton(onClick = onReset, modifier = Modifier.weight(1f)) {
                Text(strings.reset)
            }
            Button(
                onClick = onApply,
                modifier = Modifier.weight(1f),
            ) {
                Text(strings.apply)
            }
        }
    }
}

@Composable
private fun DropdownField(
    label: String,
    options: List<Pair<String, String>>,
    selected: String,
    display: (String) -> String,
    onSelect: (String) -> Unit,
) {
    var expanded by remember { mutableStateOf(false) }
    Column {
        Text(label, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        androidx.compose.foundation.layout.Box {
            OutlinedButton(
                onClick = { expanded = true },
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(8.dp),
            ) {
                Text(display(selected), maxLines = 1)
            }
            DropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
                options.forEach { (value, text) ->
                    DropdownMenuItem(
                        text = { Text(text) },
                        onClick = {
                            onSelect(value)
                            expanded = false
                        },
                    )
                }
            }
        }
    }
}

/** Horizontally scrolling chip row, one line per filter. */
@Composable
private fun ChipFlow(
    options: List<String>,
    selected: String,
    onSelect: (String) -> Unit,
    display: (String) -> String = { it },
) {
    LazyRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        items(options, key = { it }) { option ->
            TagChip(
                text = display(option),
                accent = option == selected,
                onClick = { onSelect(if (option == selected) "" else option) },
            )
        }
    }
}

/** Genre chips wrap onto as many lines as they need, unlike the single-row
 *  flows, because there are too many genres for a scroller to be usable. */
@Composable
private fun GenreGrid(
    genres: List<Genre>,
    selected: String,
    onSelect: (String) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        genres.chunked(3).forEach { row ->
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                row.forEach { genre ->
                    TagChip(
                        text = genre.display(),
                        accent = genre.slug == selected,
                        onClick = { onSelect(genre.slug) },
                        modifier = Modifier.weight(1f),
                    )
                }
                // Keep the last row aligned with the grid above it.
                repeat(3 - row.size) {
                    Spacer(Modifier.weight(1f))
                }
            }
        }
    }
}

@Composable
private fun NumberField(
    value: String,
    onValueChange: (String) -> Unit,
    placeholder: String,
    modifier: Modifier = Modifier,
) {
    OutlinedTextField(
        value = value,
        onValueChange = { text ->
            // Keep it numeric: the server parses these as i64 and a stray
            // letter would be silently dropped, so reject it at the source.
            if (text.isEmpty() || text.all { it.isDigit() }) onValueChange(text)
        },
        placeholder = { Text(placeholder) },
        singleLine = true,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
        shape = RoundedCornerShape(8.dp),
        modifier = modifier,
    )
}
