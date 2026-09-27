pluginManagement {
    repositories {
        gradlePluginPortal()
        google()
        mavenCentral()
    }
}

dependencyResolutionManagement {
    // Repositories are declared once, here, rather than per module. `PREFER_SETTINGS`
    // would make a module-level `repositories { }` a build error instead of a
    // silent override, which is what has bitten this project before.
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "AnimeDb"
include(":app")
