# ML Kit finds its parts by reflection (MlKitInitProvider instantiates its
# component registrars); R8's full mode would otherwise drop the
# constructors it calls, and text recognition never starts.
-keep class * implements com.google.firebase.components.ComponentRegistrar { <init>(); }
-keep class com.google.mlkit.** { <init>(...); }
