import java.io.File

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// The workspace root (two levels up): `crates/android` is built from there.
val repoRoot: File = rootProject.projectDir.parentFile
val rustJniLibs = layout.buildDirectory.dir("rustJniLibs")
// `-PrustRelease` builds the Rust core with the release profile (slower to build).
val rustRelease = project.hasProperty("rustRelease")

// The app's version is the workspace's (Cargo.toml): 0.8.0 -> versionName "0.8.0", code 800.
val workspaceVersion: String = Regex("""(?m)^version\s*=\s*"([^"]+)"""")
    .find(File(repoRoot, "Cargo.toml").readText())?.groupValues?.get(1) ?: "0.0.0"
val versionParts = workspaceVersion.split('.').map { it.takeWhile(Char::isDigit).toIntOrNull() ?: 0 }

// Release signing key, never in the repository: CI decodes it from secrets into a temporary
// file and passes its path and passwords through these variables (see docs/android.md).
val releaseKeystore: String? = System.getenv("BH_ANDROID_KEYSTORE")?.takeIf { File(it).isFile }

android {
    namespace = "app.blackhole.browser"
    compileSdk = 35
    ndkVersion = "27.3.13750724"

    defaultConfig {
        applicationId = "app.blackhole.browser"
        minSdk = 29
        targetSdk = 35
        versionCode = versionParts.getOrElse(0) { 0 } * 10000 + versionParts.getOrElse(1) { 0 } * 100 + versionParts.getOrElse(2) { 0 }
        versionName = workspaceVersion
    }

    signingConfigs {
        if (releaseKeystore != null) {
            create("release") {
                storeFile = File(releaseKeystore)
                storePassword = System.getenv("BH_ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("BH_ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("BH_ANDROID_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        // Debug builds use Gradle's own debug key. A release build is signed only when the
        // release key is provided (BH_ANDROID_KEYSTORE…); otherwise it is left unsigned.
        getByName("release") {
            isMinifyEnabled = false
            if (releaseKeystore != null) signingConfig = signingConfigs.getByName("release")
        }
    }

    sourceSets["main"].apply {
        // The interface pages live with the Rust core so both sides share one source of truth.
        assets.srcDirs(File(repoRoot, "crates/android/ui"))
        jniLibs.srcDirs(rustJniLibs)
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.activity:activity-ktx:1.9.3")
    implementation("androidx.webkit:webkit:1.12.1")
}

// Builds `libblackhole_android.so` for phones (arm64-v8a) and the emulator (x86_64).
val cargoNdk by tasks.registering(Exec::class) {
    group = "build"
    description = "Builds the Rust core with cargo-ndk"
    workingDir = repoRoot
    val home = System.getProperty("user.home")
    val local = File(home, ".cargo/bin/cargo" + if (System.getProperty("os.name").startsWith("Windows")) ".exe" else "")
    val cargo = if (local.exists()) local.absolutePath else "cargo"
    environment("ANDROID_NDK_HOME", android.ndkDirectory.absolutePath)
    val args = mutableListOf(
        cargo, "ndk", "-t", "arm64-v8a", "-t", "x86_64", "-P", "29",
        "-o", rustJniLibs.get().asFile.absolutePath, "build", "-p", "blackhole-android", "--locked",
    )
    if (rustRelease) args += "--release"
    commandLine(args)
    inputs.dir(File(repoRoot, "crates"))
    inputs.file(File(repoRoot, "Cargo.lock"))
    outputs.dir(rustJniLibs)
}

tasks.named("preBuild") { dependsOn(cargoNdk) }
