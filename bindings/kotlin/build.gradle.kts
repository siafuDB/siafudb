// Minimal Android library module. jniLibs/ and the Kotlin sources are filled
// by scripts/ffi/build-android.sh. Plugin versions are unverified.
plugins {
    id("com.android.library") version "8.7.3"
    id("org.jetbrains.kotlin.android") version "2.0.21"
}

android {
    namespace = "org.siafudb"
    compileSdk = 35

    defaultConfig {
        minSdk = 24
        consumerProguardFiles("consumer-rules.pro")
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
}

dependencies {
    // UniFFI-generated Kotlin loads the native library through JNA.
    implementation("net.java.dev.jna:jna:5.15.0@aar")
}
