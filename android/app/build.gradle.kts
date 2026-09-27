import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.kotlin.plugin.serialization")
}

/**
 * Reads the signing configuration, or returns null when it is absent.
 *
 * Missing signing must not break the build: a fresh clone should still be able
 * to run `assembleDebug` and produce an (unsigned) release bundle.
 */
fun loadKeystoreProperties(): Properties? {
    val file = rootProject.file("keystores/keystore.properties")
    if (!file.exists()) return null
    return Properties().apply {
        file.inputStream().use { load(it) }
    }
}

android {
    namespace = "ru.teivrim.anime"
    compileSdk = 36

    defaultConfig {
        applicationId = "ru.teivrim.anime"
        minSdk = 24
        targetSdk = 36
        versionCode = 1
        versionName = "1.0.0"

        // Overridden per build type below. The debug value points at the host
        // machine as seen from an emulator, so a fresh checkout runs with no
        // setup at all.
        buildConfigField("String", "API_BASE_URL", "\"http://10.0.2.2:8082/\"")
    }

    signingConfigs {
        if (loadKeystoreProperties() != null) {
            create("release") {
                val props = loadKeystoreProperties()!!
                storeFile = rootProject.file(props.getProperty("storeFile"))
                storePassword = props.getProperty("storePassword")
                keyAlias = props.getProperty("keyAlias")
                keyPassword = props.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        debug {
            applicationIdSuffix = ".debug"
            versionNameSuffix = "-debug"
            isMinifyEnabled = false
        }
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
            val props = loadKeystoreProperties()
            if (props != null) {
                buildConfigField("String", "API_BASE_URL", "\"${props.getProperty("apiBaseUrl")}\"")
                signingConfig = signingConfigs.getByName("release")
            } else {
                buildConfigField("String", "API_BASE_URL", "\"https://anime.teivrim.ru/\"")
                logger.warn(
                    "keystores/keystore.properties not found: the release bundle will be UNSIGNED"
                )
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    androidResources {
        // The catalogue ships in two languages. Filtering here keeps the
        // library's own strings for other locales out of the APK.
        localeFilters += listOf("ru", "en")
    }

    packaging {
        resources.excludes += setOf(
            "/META-INF/{AL2.0,LGPL2.1}",
            "/META-INF/DEPENDENCIES",
            "DebugProbesKt.bin",
            "kotlin-tooling-metadata.json",
        )
    }

    lint {
        abortOnError = true
        warningsAsErrors = false
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2026.02.01"))

    // AndroidX pins the `minCompileSdk` of every AAR, and AGP refuses to build
    // when a dependency asks for a newer platform than the project targets. The
    // newest train (core-ktx 1.19, lifecycle 2.11) requires compileSdk 37 and
    // AGP 9.1, so these are the newest releases that still build on AGP 8.13 +
    // compileSdk 36. Bumping either of the two requires bumping the other.
    implementation("androidx.core:core-ktx:1.16.0")
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.9.4")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.9.4")
    implementation("androidx.navigation:navigation-compose:2.8.0")

    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-graphics")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3:1.4.0")
    // `material-icons-core` only ships the Filled set. The outlined star is the
    // "not a favourite" affordance and has no core equivalent, and R8 drops
    // everything the app does not reference out of the extended artifact.
    implementation("androidx.compose.material:material-icons-extended")

    implementation("io.coil-kt:coil-compose:2.7.0")

    implementation("com.squareup.okhttp3:okhttp:4.12.0")
    implementation("com.squareup.okhttp3:logging-interceptor:4.12.0")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.9.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")

    implementation("androidx.datastore:datastore-preferences:1.1.1")

    debugImplementation("androidx.compose.ui:ui-tooling")

    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.6.1")
    androidTestImplementation(platform("androidx.compose:compose-bom:2026.02.01"))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
}
