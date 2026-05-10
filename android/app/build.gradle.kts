// :app — Xboard Android client.
//
// Two non-trivial bits worth knowing:
//
//   1. UniFFI Kotlin bindings are *generated*, not committed. The
//      `generateUniffiBindings` task below shells out to the in-tree
//      `uniffi-bindgen` cargo bin (see core/src/bin/uniffi_bindgen.rs)
//      against `core/src/ffi.udl`, and the resulting Kotlin source lands
//      under `src/main/kotlin/com/xboard/client/core/` (path matches the
//      package_name in core/uniffi.toml). The .gitignore at android/
//      excludes that dir.
//
//   2. `libxboard_core.so` (the Rust dylib UniFFI loads via JNA) and the
//      bundled `libmihomo.so` (mihomo binary disguised as a .so to bypass
//      Android 12+ exec restrictions) are produced by `just core-android`
//      and `ci/scripts/install-mihomo-android.sh` respectively. Both land
//      under `src/main/jniLibs/<abi>/` — also gitignored.

plugins {
    alias(libs.plugins.androidApplication)
    alias(libs.plugins.kotlinAndroid)
    alias(libs.plugins.kotlinComposeCompiler)
    alias(libs.plugins.kotlinSerialization)
}

android {
    namespace = "com.xboard.client"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.xboard.client"
        minSdk = 24
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"

        // Default backend URL baked into the APK. Overridable at runtime
        // by writing to the "xboard.backend_base_url" key in the secure
        // store (used by QA / staging — no in-app UI for it).
        // Set via gradle.properties: `xboard.defaultBackendUrl=...`
        // or env var `XBOARD_DEFAULT_BACKEND_URL`.
        val defaultBackend = (project.findProperty("xboard.defaultBackendUrl") as String?)
            ?: System.getenv("XBOARD_DEFAULT_BACKEND_URL")
            ?: "https://your-xboard-panel.example.com"
        buildConfigField("String", "DEFAULT_BACKEND_URL", "\"$defaultBackend\"")

        ndk {
            // Three ABIs match the `cargo ndk` invocation in justfile's
            // `core-android` recipe. x86_64 is emulator-only; we keep it
            // for dev convenience but strip it from release builds via
            // packagingOptions if needed.
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64")
        }

        vectorDrawables { useSupportLibrary = true }
    }

    buildTypes {
        debug {
            isMinifyEnabled = false
            applicationIdSuffix = ".debug"
            versionNameSuffix = "-debug"
        }
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
            // Signing config is injected by CI (see .github/workflows/mobile.yml).
            // For local release builds, copy keystore.properties.example to
            // keystore.properties and uncomment the signingConfig hookup below.
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    sourceSets {
        getByName("main") {
            // jniLibs dir is the default (`src/main/jniLibs`); declared
            // explicitly so it's obvious where `cargo ndk` and
            // `install-mihomo-android.sh` are expected to write.
            jniLibs.srcDirs("src/main/jniLibs")

            // Generated UniFFI bindings live alongside hand-written Kotlin.
            // Listed explicitly so a fresh checkout (with the `core/` dir
            // empty per .gitignore) still resolves once `generateUniffiBindings`
            // runs.
            kotlin.srcDirs("src/main/kotlin")
        }
    }

    packaging {
        resources {
            // Compose / kotlinx pull in a few overlapping META-INF entries.
            excludes += listOf(
                "/META-INF/{AL2.0,LGPL2.1}",
                "/META-INF/INDEX.LIST",
                "/META-INF/io.netty.versions.properties",
            )
        }
        jniLibs {
            // mihomo (renamed libmihomo.so) lives in jniLibs but must
            // ship uncompressed so it can be exec'd at runtime — the
            // default `useLegacyPackaging = false` already gives us this
            // on AGP 8, but make it explicit for future readers.
            useLegacyPackaging = false
        }
    }

    // Surface lint warnings at build time, but don't fail the build —
    // we have detekt for the strict pass (M6 CI).
    lint {
        abortOnError = false
        warningsAsErrors = false
    }
}

dependencies {
    implementation(platform(libs.androidx.compose.bom))

    // Core / lifecycle / activity
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.appcompat)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.lifecycle.viewmodel.ktx)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)

    // Compose
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.ui.graphics)
    implementation(libs.androidx.compose.ui.tooling.preview)
    debugImplementation(libs.androidx.compose.ui.tooling)
    implementation(libs.androidx.compose.foundation)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.material.icons.extended)
    implementation(libs.androidx.navigation.compose)

    // Persistence
    implementation(libs.androidx.security.crypto)
    implementation(libs.androidx.datastore.preferences)

    // Coroutines
    implementation(libs.kotlinx.coroutines.android)

    // UniFFI Kotlin bindings load the .so via JNA on Android.
    // Pulling `@aar` resolves the bundled native shims so we don't have
    // to ship our own.
    implementation(libs.jna) { artifact { type = "aar" } }

    // kotlinx-serialization for cache-side helpers (e.g. decoding
    // `data_json` blobs out of CheckoutResponse). The wire layer is
    // already JSON-decoded inside Rust.
    implementation(libs.kotlinx.serialization.json)
}

// ---------------------------------------------------------------------------
// UniFFI Kotlin binding generation
// ---------------------------------------------------------------------------
// We use the in-tree `uniffi-bindgen` cargo bin (declared in core/Cargo.toml)
// rather than `cargo install uniffi-bindgen-cli` — the standalone CLI crate
// stopped publishing at 0.28 so the in-tree route is canonical now.

val uniffiOutDir = layout.projectDirectory.dir("src/main/kotlin")
val workspaceRoot = rootProject.projectDir.parentFile  // android/.. == repo root

val generateUniffiBindings = tasks.register<Exec>("generateUniffiBindings") {
    group = "uniffi"
    description = "Generate Kotlin bindings from core/src/ffi.udl via the in-tree uniffi-bindgen."

    workingDir = workspaceRoot
    commandLine(
        "cargo", "run",
        "-p", "xboard-core",
        "--bin", "uniffi-bindgen",
        "--",
        "generate",
        "core/src/ffi.udl",
        "--language", "kotlin",
        "--out-dir", uniffiOutDir.asFile.absolutePath,
    )

    inputs.file(workspaceRoot.resolve("core/src/ffi.udl"))
    inputs.file(workspaceRoot.resolve("core/uniffi.toml"))
    outputs.dir(uniffiOutDir.dir("com/xboard/client/core"))
}

androidComponents {
    onVariants { variant ->
        variant.sources.kotlin
            ?.addStaticSourceDirectory(uniffiOutDir.asFile.absolutePath)
    }
}

tasks.named("preBuild").configure {
    dependsOn(generateUniffiBindings)
}
