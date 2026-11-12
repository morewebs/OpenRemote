# Add project specific ProGuard rules here.
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

# If your project uses WebView with JS, uncomment the following
# and specify the fully qualified class name to the JavaScript interface
# class:
#-keepclassmembers class fqcn.of.javascript.interface.for.webview {
#   public *;
#}

# Uncomment this to preserve the line number information for
# debugging stack traces.
#-keepattributes SourceFile,LineNumberTable

# If you keep the line number information, uncomment this to
# hide the original source file name.
#-renamesourcefileattribute SourceFile

# The built-in browser's plugin: Tauri makes it by its class name over JNI,
# finds its commands by method name, and reads the start command's
# arguments (the events channel) into StartArgs's fields by reflection.
-keep class space.moreweb.openremote.browser.BrowserPlugin {
  public <init>(...);
  @app.tauri.annotation.Command public <methods>;
}
-keep class space.moreweb.openremote.browser.StartArgs { *; }
