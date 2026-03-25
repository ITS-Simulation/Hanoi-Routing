plugins {
    alias(libs.plugins.kotlin.jvm) apply false
    alias(libs.plugins.kotlin.serialization) apply false
    alias(libs.plugins.shadow) apply false
}

allprojects {
    group = "com.thomas"
    version = "0.0.1"
    repositories {
        mavenCentral()
    }
}