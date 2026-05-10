# UniFFI Kotlin bindings call into the .so via JNA. JNA needs reflection
# access to its proxy classes, and our generated bindings use callback
# interfaces (StateObserver / SecureStore / TunDelegate) whose method
# names must survive minification or the FFI bridge breaks at runtime.

-keep class com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.** { *; }

# Generated UniFFI bindings package — preserve everything under it.
-keep class com.xboard.client.core.** { *; }
-keepclassmembers class com.xboard.client.core.** { *; }

# Our own callback interface implementations live under .vpn / .data /
# .secure — the UniFFI runtime invokes them via JNA proxies, so their
# method signatures must not be renamed.
-keep class com.xboard.client.vpn.** { *; }
-keep class com.xboard.client.data.** { *; }
-keep class com.xboard.client.secure.** { *; }

# kotlinx-serialization needs the @Serializable companion + serializer().
-keepattributes *Annotation*, InnerClasses
-dontnote kotlinx.serialization.AnnotationsKt
-keepclassmembers class kotlinx.serialization.json.** {
    *** Companion;
}
-keepclasseswithmembers class kotlinx.serialization.json.** {
    kotlinx.serialization.KSerializer serializer(...);
}

# Keep our own @Serializable types (we prefix them with .data.)
-keep,includedescriptorclasses class com.xboard.client.data.**$$serializer { *; }
-keepclassmembers class com.xboard.client.data.** {
    *** Companion;
    kotlinx.serialization.KSerializer serializer(...);
}

# Compose runtime keeps itself, but suppress noisy warnings.
-dontwarn org.jetbrains.compose.**
-dontwarn androidx.compose.**
