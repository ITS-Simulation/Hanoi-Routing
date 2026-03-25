plugins {
    alias(libs.plugins.kotlin.jvm)
    alias(libs.plugins.kotlin.serialization)
    alias(libs.plugins.shadow)
    application
}

group = "com.thomas"
version = "0.0.1"

repositories {
    mavenCentral()
}

dependencies {
    testImplementation(kotlin("test"))
    implementation(project(":modeler"))
    implementation(project(":smoother"))
    implementation(project(":simulation"))
    implementation(libs.clikt)
}

kotlin {
    jvmToolchain(21)
}

application {
    mainClass.set("com.thomas.cch_app.MainKt")
}

tasks.test {
    useJUnitPlatform()
}

tasks.shadowJar {
    manifest {
        attributes["Main-Class"] = application.mainClass
    }
    archiveBaseName.set("cch")
    archiveClassifier.set("")
}