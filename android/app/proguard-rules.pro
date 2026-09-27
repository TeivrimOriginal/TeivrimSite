# R8 / ProGuard rules for the release build.
#
# The app is almost entirely Compose plus kotlinx.serialization, so the default
# rules cover nearly all of it. What is left are the cases where R8 cannot see
# a reference and would otherwise strip or rename something that is used
# reflectively.

# --- kotlinx.serialization -------------------------------------------------
# The plugin generates a `Companion.serializer()` for every @Serializable class
# and looks it up by name, which R8 cannot trace.
-keepattributes *Annotation*, InnerClasses
-dontnote kotlinx.serialization.**

-keepclassmembers class ru.teivrim.anime.data.** {
    *** Companion;
}
-keepclasseswithmembers class ru.teivrim.anime.data.** {
    kotlinx.serialization.KSerializer serializer(...);
}

# The generated serializers are instantiated reflectively by name.
-keep class ru.teivrim.anime.data.**$$serializer { *; }

# --- OkHttp ----------------------------------------------------------------
# OkHttp references optional Conscrypt/BouncyCastle providers that are not on
# the classpath; R8 warns about them and they are not needed.
-dontwarn okhttp3.internal.platform.**
-dontwarn org.conscrypt.**
-dontwarn org.bouncycastle.**
-dontwarn org.openjsse.**

# --- Kotlin coroutines -----------------------------------------------------
-dontwarn kotlinx.coroutines.**

# --- Compose ---------------------------------------------------------------
# Keep the runtime's own rules; only silence warnings about optional features.
-dontwarn androidx.compose.ui.tooling.**

# Keep source line numbers for readable crash reports from RuStore, but hide
# the original file names.
-keepattributes SourceFile,LineNumberTable
-renamesourcefileattribute SourceFile
