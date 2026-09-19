plugins {
    id("com.android.application")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

android {
    namespace = "com.openremote.companion"
    compileSdk = flutter.compileSdkVersion
    ndkVersion = flutter.ndkVersion

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        applicationId = "com.openremote.companion"
        // You can update the following values to match your application needs.
        // For more information, see: https://flutter.dev/to/review-gradle-config.
        minSdk = flutter.minSdkVersion
        targetSdk = flutter.targetSdkVersion
        versionCode = flutter.versionCode
        versionName = flutter.versionName
    }

    signingConfigs {
        val keyStorePath = System.getenv("OPENREMOTE_KEYSTORE")
        if (!keyStorePath.isNullOrBlank()) {
            create("distribution") {
                storeFile = file(keyStorePath)
                storePassword = System.getenv("OPENREMOTE_STORE_PASSWORD")
                keyAlias = System.getenv("OPENREMOTE_KEY_ALIAS")
                keyPassword = System.getenv("OPENREMOTE_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            // Local/CI smoke builds use a development key. Distribution builds
            // provide OPENREMOTE_KEYSTORE and the associated credentials.
            signingConfig = signingConfigs.findByName("distribution")
                ?: signingConfigs.getByName("debug")
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

flutter {
    source = "../.."
}
