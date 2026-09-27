// Root build file. Plugins are declared here with `apply false` so each module
// opts in, which keeps the versions in one place.
//
// AGP 8.x rather than 9.x on purpose: AGP 9 registers its own `kotlin`
// extension, and applying org.jetbrains.kotlin.android on top of that fails
// with "Cannot add extension with name 'kotlin'". The separate Kotlin plugin is
// what the Compose and serialization sub-plugins hang off, so 8.x is the
// combination that works without workarounds.
plugins {
    id("com.android.application") version "8.13.0" apply false
    id("org.jetbrains.kotlin.android") version "2.2.10" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.2.10" apply false
    id("org.jetbrains.kotlin.plugin.serialization") version "2.2.10" apply false
}
