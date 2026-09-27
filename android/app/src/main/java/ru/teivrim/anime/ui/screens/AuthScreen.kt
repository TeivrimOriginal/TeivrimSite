package ru.teivrim.anime.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
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
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import ru.teivrim.anime.BuildConfig
import ru.teivrim.anime.data.AppLanguage
import ru.teivrim.anime.data.ThemeMode
import ru.teivrim.anime.ui.AuthState
import ru.teivrim.anime.ui.AuthViewModel
import ru.teivrim.anime.ui.Strings
import ru.teivrim.anime.ui.components.SectionTitle

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AuthScreen(
    vm: AuthViewModel,
    strings: Strings,
    onDone: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var signUp by remember { mutableStateOf(false) }
    var username by remember { mutableStateOf("") }
    var email by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var confirm by remember { mutableStateOf("") }
    val state by vm.state.collectAsStateWithLifecycle()

    Column(
        modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Spacer(Modifier.height(24.dp))
        Text(strings.account, style = MaterialTheme.typography.headlineSmall)

        Spacer(Modifier.height(20.dp))

        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
            SegmentedButton(
                selected = !signUp,
                onClick = { signUp = false; vm.reset() },
                shape = SegmentedButtonDefaults.itemShape(index = 0, count = 2),
            ) { Text(strings.signIn) }
            SegmentedButton(
                selected = signUp,
                onClick = { signUp = true; vm.reset() },
                shape = SegmentedButtonDefaults.itemShape(index = 1, count = 2),
            ) { Text(strings.signUp) }
        }

        Spacer(Modifier.height(20.dp))

        OutlinedTextField(
            value = username,
            onValueChange = { username = it },
            label = { Text(if (signUp) strings.username else strings.loginOrEmail) },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )

        if (signUp) {
            Spacer(Modifier.height(10.dp))
            OutlinedTextField(
                value = email,
                onValueChange = { email = it },
                label = { Text(strings.emailOptional) },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
                modifier = Modifier.fillMaxWidth(),
            )
        }

        Spacer(Modifier.height(10.dp))
        OutlinedTextField(
            value = password,
            onValueChange = { password = it },
            label = { Text(strings.password) },
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
            supportingText = if (signUp) {
                { Text(strings.passwordHint) }
            } else {
                null
            },
            modifier = Modifier.fillMaxWidth(),
        )

        if (signUp) {
            Spacer(Modifier.height(10.dp))
            OutlinedTextField(
                value = confirm,
                onValueChange = { confirm = it },
                label = { Text(strings.password) },
                singleLine = true,
                visualTransformation = PasswordVisualTransformation(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                modifier = Modifier.fillMaxWidth(),
            )
        }

        when (val s = state) {
            is AuthState.Failed -> {
                Spacer(Modifier.height(12.dp))
                Text(
                    text = s.message,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                )
            }

            is AuthState.Invalid -> {
                Spacer(Modifier.height(12.dp))
                Text(
                    text = when (s.error) {
                        ru.teivrim.anime.ui.AuthError.ShortUsername -> strings.usernameTooShort
                        ru.teivrim.anime.ui.AuthError.PasswordMismatch -> strings.passwordMismatch
                        ru.teivrim.anime.ui.AuthError.EmptyPassword -> strings.passwordHint
                    },
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                )
            }

            else -> Unit
        }

        Spacer(Modifier.height(20.dp))
        Button(
            onClick = {
                vm.submit(signUp, username, email, password, confirm) { onDone() }
            },
            enabled = state !is AuthState.Busy,
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 50.dp),
        ) {
            if (state is AuthState.Busy) {
                CircularProgressIndicator(
                    Modifier.height(18.dp),
                    strokeWidth = 2.dp,
                    color = MaterialTheme.colorScheme.onPrimary,
                )
            } else {
                Text(if (signUp) strings.signUp else strings.signIn)
            }
        }

        Spacer(Modifier.height(12.dp))
        TextButton(onClick = { signUp = !signUp; vm.reset() }) {
            Text(if (signUp) strings.haveAccount else strings.noAccount)
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    strings: Strings,
    language: AppLanguage,
    theme: ThemeMode,
    russianFirst: Boolean,
    username: String?,
    onLanguage: (AppLanguage) -> Unit,
    onTheme: (ThemeMode) -> Unit,
    onRussianFirst: (Boolean) -> Unit,
    onSignOut: () -> Unit,
    onSignIn: () -> Unit,
) {
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(24.dp),
    ) {
        if (username != null) {
            Text(username, style = MaterialTheme.typography.headlineSmall)
            Spacer(Modifier.height(6.dp))
            OutlinedButton(onClick = onSignOut, modifier = Modifier.fillMaxWidth()) {
                Text(strings.signOut)
            }
            Spacer(Modifier.height(28.dp))
        } else {
            Button(onClick = onSignIn, modifier = Modifier.fillMaxWidth()) {
                Text(strings.signIn)
            }
            Spacer(Modifier.height(28.dp))
        }

        SectionTitle(strings.language)
        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
            AppLanguage.entries.forEachIndexed { index, option ->
                SegmentedButton(
                    selected = language == option,
                    onClick = { onLanguage(option) },
                    shape = SegmentedButtonDefaults.itemShape(index = index, count = AppLanguage.entries.size),
                ) { Text(if (option == AppLanguage.Russian) "RU" else "EN") }
            }
        }
        Spacer(Modifier.height(20.dp))

        SectionTitle(strings.theme)
        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
            ThemeMode.entries.forEachIndexed { index, option ->
                SegmentedButton(
                    selected = theme == option,
                    onClick = { onTheme(option) },
                    shape = SegmentedButtonDefaults.itemShape(index = index, count = ThemeMode.entries.size),
                ) {
                    Text(
                        when (option) {
                            ThemeMode.System -> strings.themeSystem
                            ThemeMode.Light -> strings.themeLight
                            ThemeMode.Dark -> strings.themeDark
                        },
                        maxLines = 1,
                    )
                }
            }
        }
        Spacer(Modifier.height(20.dp))

        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = 48.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Text(strings.russianTitles, style = MaterialTheme.typography.bodyMedium)
                Text(
                    strings.catalog,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Switch(checked = russianFirst, onCheckedChange = onRussianFirst)
        }

        Spacer(Modifier.height(28.dp))
        SectionTitle(strings.server)
        Text(
            text = BuildConfig.API_BASE_URL,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(6.dp))
        Text(
            text = "${strings.version}: ${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE})",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
