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
    ndkVersion = "27.3.13750724"

    defaultConfig {
        applicationId = "com.xboard.client"
        minSdk = 24
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"

        // API endpoints and feature policy are embedded in the Rust core
        // through SUFE_DEPLOYMENT_JSON. AppContainer uses Client.forDeployment.

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
            // R8 / minify disabled for the v0.1.x line — JNA pulls java.awt.*
            // and Tink pulls javax.annotation.* references that R8 flags as
            // missing classes. Both are runtime-irrelevant on Android, but
            // exhausting the -dontwarn list takes more iteration than the
            // ~15 MB APK savings buy us right now. Re-enable once a proper
            // proguard-rules.pro is in place (M6+).
            isMinifyEnabled = false
            isShrinkResources = false
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
            // be extracted to nativeLibraryDir to be exec'd. Loading
            // directly from an uncompressed APK does not create that file.
            useLegacyPackaging = true
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

    // Material Components for Android — required because res/values/themes.xml
    // references Theme.Material3.DayNight.NoActionBar, which is an XML-only
    // style not bundled with compose-material3.
    implementation(libs.material)

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

    // Render payment QR payloads locally; no third-party image service.
    implementation("com.google.zxing:core:3.5.3")
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

        val variantName = variant.name.replaceFirstChar { it.uppercaseChar() }
        val requiredNativeFiles = listOf("arm64-v8a", "armeabi-v7a", "x86_64").flatMap { abi ->
            listOf("libxboard_core.so", "libmihomo.so").map { library ->
                "$abi/$library" to project.file("src/main/jniLibs/$abi/$library")
            }
        }
        val verifyNativeLibraries = tasks.register("verify${variantName}NativeLibraries") {
            group = "verification"
            description = "Refuse to package an APK without its Rust core and mihomo executables."
            doLast {
                for ((relativePath, binary) in requiredNativeFiles) {
                        check(binary.isFile && binary.length() > 4) {
                            "Missing $relativePath. Run just core-android and just kernel-android before packaging."
                        }
                        val header = binary.inputStream().use { it.readNBytes(4) }
                        check(header.contentEquals(byteArrayOf(0x7f, 0x45, 0x4c, 0x46))) {
                            "Invalid ELF binary: $relativePath. Rebuild or download the verified native artifact."
                        }
                }
            }
        }
        tasks.matching { it.name == "merge${variantName}NativeLibs" }.configureEach {
            dependsOn(verifyNativeLibraries)
        }
    }
}

tasks.named("preBuild").configure {
    dependsOn(generateUniffiBindings)
}
