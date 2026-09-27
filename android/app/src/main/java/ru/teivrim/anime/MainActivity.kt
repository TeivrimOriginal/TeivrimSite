package ru.teivrim.anime

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalUriHandler
import androidx.core.net.toUri
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation.NavHostController
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import kotlinx.coroutines.launch
import ru.teivrim.anime.data.AnimeRepository
import ru.teivrim.anime.data.AppLanguage
import ru.teivrim.anime.data.ThemeMode
import ru.teivrim.anime.ui.AuthViewModel
import ru.teivrim.anime.ui.CatalogViewModel
import ru.teivrim.anime.ui.DetailViewModel
import ru.teivrim.anime.ui.ListViewModel
import ru.teivrim.anime.ui.Strings
import ru.teivrim.anime.ui.screens.AuthScreen
import ru.teivrim.anime.ui.screens.CatalogScreen
import ru.teivrim.anime.ui.screens.DetailScreen
import ru.teivrim.anime.ui.screens.SettingsScreen
import ru.teivrim.anime.ui.screens.WatchlistScreen
import ru.teivrim.anime.ui.theme.AnimeTheme

class MainActivity : ComponentActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)

        // A teivrim-anime:// or /anime/<uid> link should land on the title.
        val initialUid = intent?.data?.uidFromUri()

        setContent {
            App(initialUid = initialUid)
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
    }
}

/**
 * Pulls a uid out of an incoming link.
 *
 * The custom scheme is written `teivrim-anime:al:16498`, and `Uri`'s own
 * authority parsing mangles a non-numeric port, so the raw string is used there
 * rather than `host`. The web form is `https://<host>/anime/<uid>`, where the uid
 * is the second path segment.
 */
private fun Uri.uidFromUri(): String? {
    if (scheme == "teivrim-anime") {
        return toString()
            .substringAfter("teivrim-anime:", "")
            .trim('/')
            .substringBefore('?')
            .ifEmpty { null }
    }
    return pathSegments.drop(1).firstOrNull()?.substringBefore('?')?.ifEmpty { null }
}

@Composable
fun App(initialUid: String? = null) {
    val app = AnimeApp.instance
    val scope = rememberCoroutineScope()

    var language by remember { mutableStateOf(AppLanguage.Russian) }
    var theme by remember { mutableStateOf(ThemeMode.System) }
    var russianFirst by remember { mutableStateOf(true) }
    var username by remember { mutableStateOf<String?>(null) }
    var tokenVersion by rememberSaveable { mutableIntStateOf(0) }

    LaunchedEffect(Unit) {
        app.session.languageFlow.collect { language = it }
    }
    LaunchedEffect(Unit) {
        app.session.themeFlow.collect { theme = it }
    }
    LaunchedEffect(Unit) {
        app.session.russianFirstFlow.collect { russianFirst = it }
    }
    LaunchedEffect(tokenVersion, app.session.cachedToken) {
        // Re-check the stored token on every auth change so a revoked session
        // drops the cached profile instead of showing a stale username.
        username = app.session.cachedToken?.let { app.repository.refreshSession()?.username }
    }

    AnimeTheme(themeMode = theme) {
        Surface {
            val strings = remember(language) { Strings(language) }
            NavGraph(
                strings = strings,
                russianFirst = russianFirst,
                language = language,
                theme = theme,
                username = username,
                initialUid = initialUid,
                onSignedIn = { tokenVersion++ },
                onSignedOut = {
                    tokenVersion++
                    username = null
                },
                onSettingChange = { scope.launch { it() } },
            )
        }
    }
}

/**
 * Navigation.
 *
 * A uid looks like `al:16498`, so it is a path segment rather than a query
 * parameter: a route stays a route, and the colon never reaches a query parser.
 *
 * Three top-level destinations share a bottom bar — catalogue, list, settings.
 * The bar is hidden on the detail and auth screens, which are pushed on top of
 * a tab and have their own back navigation.
 */
@Composable
private fun NavGraph(
    strings: Strings,
    russianFirst: Boolean,
    language: AppLanguage,
    theme: ThemeMode,
    username: String?,
    initialUid: String?,
    onSignedIn: () -> Unit,
    onSignedOut: () -> Unit,
    onSettingChange: (suspend () -> Unit) -> Unit,
) {
    val nav = rememberNavController()
    val app = AnimeApp.instance
    val context = LocalContext.current
    val uriHandler = LocalUriHandler.current

    val start = if (initialUid != null) "detail/$initialUid" else "catalog"

    val backStack by nav.currentBackStackEntryAsState()
    val route = backStack?.destination?.route
    val topLevel = TOP_LEVEL.firstOrNull { it.route == route }

    Scaffold(
        bottomBar = {
            if (topLevel != null) {
                NavigationBar {
                    TOP_LEVEL.forEach { item ->
                        NavigationBarItem(
                            selected = route == item.route,
                            onClick = { nav.switchTab(item.route) },
                            icon = { Icon(item.icon, null) },
                            label = { Text(strings.tabLabel(item)) },
                        )
                    }
                }
            }
        },
    ) { outer ->
        NavHost(
            navController = nav,
            startDestination = start,
            modifier = Modifier.padding(outer),
        ) {
            composable("catalog") {
                val vm: CatalogViewModel = viewModel(factory = repoFactory { CatalogViewModel(app.repository) })
                CatalogScreen(
                    vm = vm,
                    strings = strings,
                    russianFirst = russianFirst,
                    coverUrl = app::coverUrl,
                    onOpen = { nav.navigate("detail/$it") },
                    onSignInRequired = { nav.navigate("auth") },
                )
            }

            composable(
                route = "detail/{uid}",
                arguments = listOf(navArgument("uid") { type = NavType.StringType }),
            ) { entry ->
                val uid = entry.arguments?.getString("uid").orEmpty()
                val vm: DetailViewModel = viewModel(
                    factory = repoFactory { DetailViewModel(app.repository) },
                )
                DetailScreen(
                    uid = uid,
                    vm = vm,
                    strings = strings,
                    russianFirst = russianFirst,
                    coverUrl = app::coverUrl,
                    onBack = { nav.popBackStack() },
                    onOpen = { nav.navigate("detail/$it") },
                    onOpenUrl = { link ->
                        openExternally(context, link, strings.linkFailed, uriHandler::openUri)
                    },
                    onSignInRequired = { nav.navigate("auth") },
                )
            }

            composable("watchlist") {
                val vm: ListViewModel = viewModel(factory = repoFactory { ListViewModel(app.repository) })
                WatchlistScreen(
                    vm = vm,
                    strings = strings,
                    russianFirst = russianFirst,
                    coverUrl = app::coverUrl,
                    onBack = { nav.popBackStack() },
                    onOpen = { nav.navigate("detail/$it") },
                    onSignInRequired = { nav.navigate("auth") },
                )
            }

            composable("auth") {
                val vm: AuthViewModel = viewModel(factory = repoFactory { AuthViewModel(app.repository) })
                AuthScreen(
                    vm = vm,
                    strings = strings,
                    onDone = {
                        onSignedIn()
                        nav.popBackStack()
                    },
                )
            }

            composable("settings") {
                SettingsScreen(
                    strings = strings,
                    language = language,
                    theme = theme,
                    russianFirst = russianFirst,
                    username = username,
                    onLanguage = { onSettingChange { app.session.setLanguage(it) } },
                    onTheme = { onSettingChange { app.session.setTheme(it) } },
                    onRussianFirst = { onSettingChange { app.session.setRussianFirst(it) } },
                    onSignIn = { nav.navigate("auth") },
                    onSignOut = {
                        onSettingChange { app.repository.logout() }
                        onSignedOut()
                    },
                )
            }
        }
    }
}

/** The tabs, in bar order. */
private val TOP_LEVEL = listOf(
    Tab("catalog", Icons.Filled.Home),
    Tab("watchlist", Icons.Filled.Favorite),
    Tab("settings", Icons.Filled.Settings),
)

private data class Tab(val route: String, val icon: ImageVector)

private fun Strings.tabLabel(tab: Tab): String = when (tab.route) {
    "catalog" -> catalog
    "watchlist" -> myList
    else -> settings
}

/**
 * Tab switching without stacking duplicates.
 *
 * `popUpTo(startDestination) { saveState = true }` keeps each tab's scroll
 * position and back stack, so coming back to the catalogue lands where it was
 * left rather than at the top of page one.
 */
private fun NavHostController.switchTab(route: String) {
    navigate(route) {
        popUpTo(graph.startDestinationId) { saveState = true }
        launchSingleTop = true
        restoreState = true
    }
}

/** Falls back to the system browser when no app claims the URL. */
private fun openExternally(
    context: Context,
    link: String,
    failureMessage: String,
    openWith: (String) -> Unit,
) {
    try {
        openWith(link)
    } catch (e: Exception) {
        runCatching { context.startActivity(Intent(Intent.ACTION_VIEW, link.toUri())) }
            .onFailure {
                Toast.makeText(context, failureMessage, Toast.LENGTH_SHORT).show()
            }
    }
}

/** Builds a ViewModel factory for a constructor that takes the repository. */
private fun repoFactory(create: (AnimeRepository) -> ViewModel): ViewModelProvider.Factory =
    object : ViewModelProvider.Factory {
        @Suppress("UNCHECKED_CAST")
        override fun <T : ViewModel> create(modelClass: Class<T>): T =
            create(AnimeApp.instance.repository) as T
    }
