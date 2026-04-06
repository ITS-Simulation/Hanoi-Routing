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
    implementation(libs.yaml)
    implementation(libs.arrow.vector)
    implementation(libs.arrow.memory)
    implementation(libs.arrow.memory.netty)
    implementation(libs.apache.log4j)
    implementation(libs.ktor.client.core)
    implementation(libs.ktor.client.cio)
    implementation(libs.kotlinx.coroutines.core)
}

kotlin {
    jvmToolchain(21)
}

application {
    mainClass.set("com.thomas.cch_app.MainKt")
    applicationDefaultJvmArgs = listOf(
        "--add-opens=java.base/java.nio=org.apache.arrow.memory.core,ALL-UNNAMED"
    )
}

tasks.test {
    useJUnitPlatform()
    jvmArgs("--add-opens=java.base/java.nio=org.apache.arrow.memory.core,ALL-UNNAMED")
}

tasks.shadowJar {
    manifest {
        attributes["Main-Class"] = application.mainClass
        attributes["Add-Opens"] =
            "java.base/java.nio=org.apache.arrow.memory.core,ALL-UNNAMED"
    }
    archiveBaseName.set("cch")
    archiveClassifier.set("")
}
