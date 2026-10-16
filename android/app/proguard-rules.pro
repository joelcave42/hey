# Google's code scanner (ML Kit) finds its parts by name at runtime, so R8
# can't see they're used and strips them, which crashed "Scan pairing code"
# in release builds. Keep them.
-keep class com.google.mlkit.** { *; }
-keep class com.google.android.gms.internal.mlkit_code_scanner.** { *; }
-keep class * implements com.google.firebase.components.ComponentRegistrar { *; }
