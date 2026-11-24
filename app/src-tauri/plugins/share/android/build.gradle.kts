import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "io.felixbennett.paperloom.share"
    compileSdk = 36

    defaultConfig {
        minSdk = 24
        // Rules the app's shrinker applies to this library (ML Kit).
        consumerProguardFiles("consumer-rules.pro")
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

dependencies {
    implementation(project(":tauri-android"))
    // Text in pictures of scanned pages, on the phone (Chinese and Latin).
    implementation("com.google.mlkit:text-recognition-chinese:16.0.1")
}
