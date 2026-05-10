// Root build script — only declares the plugins the app module uses, with
// `apply false` so they're resolved once and applied per-subproject. Avoids
// the legacy `buildscript {}` block entirely.

plugins {
    alias(libs.plugins.androidApplication) apply false
    alias(libs.plugins.kotlinAndroid) apply false
    alias(libs.plugins.kotlinComposeCompiler) apply false
    alias(libs.plugins.kotlinSerialization) apply false
}
