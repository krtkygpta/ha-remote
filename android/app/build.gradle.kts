import java.io.FileInputStream
import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// Release signing comes from a gitignored keystore.properties at the android/ root
// (storeFile, storePassword, keyAlias, keyPassword). Without it, release builds stay unsigned.
val keystorePropsFile = rootProject.file("keystore.properties")
val keystoreProps = Properties().apply {
    if (keystorePropsFile.exists()) FileInputStream(keystorePropsFile).use { load(it) }
}

android {
    namespace = "dev.cmf.ha"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.cmf.ha"
        minSdk = 26
        targetSdk = 35
        versionCode = 18
        versionName = "0.4.4"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions { jvmTarget = "17" }

    signingConfigs {
        if (keystorePropsFile.exists()) {
            create("release") {
                storeFile = rootProject.file(keystoreProps.getProperty("storeFile"))
                storePassword = keystoreProps.getProperty("storePassword")
                keyAlias = keystoreProps.getProperty("keyAlias")
                keyPassword = keystoreProps.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            if (keystorePropsFile.exists()) {
                signingConfig = signingConfigs.getByName("release")
            }
        }
    }
}

// Refresh the bundled installer payload from the workspace's latest watch build when present.
val syncBundledWatchApp = tasks.register("syncBundledWatchApp") {
    doLast {
        val packageFile = rootProject.file("../../../../../../target/ha-remote.wp2app")
        if (packageFile.isFile) {
            val assetsDir = layout.projectDirectory.dir("src/main/assets").asFile
            assetsDir.mkdirs()
            packageFile.copyTo(assetsDir.resolve("ha-remote.wp2app"), overwrite = true)
        }
    }
}
tasks.named("preBuild").configure { dependsOn(syncBundledWatchApp) }

dependencies {
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("com.google.android.material:material:1.12.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
    testImplementation("junit:junit:4.13.2")
}
