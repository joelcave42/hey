plugins {
    id("com.android.application")
}

android {
    namespace = "io.github.joelcave42.hey"
    compileSdk = 36

    defaultConfig {
        applicationId = "io.github.joelcave42.hey"
        minSdk = 26
        targetSdk = 36
        versionCode = 2
        versionName = "0.2.0"
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"))
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation("androidx.appcompat:appcompat:1.8.0")
    implementation("com.google.android.material:material:1.14.0")
    implementation("com.google.android.gms:play-services-code-scanner:16.1.0")
}
