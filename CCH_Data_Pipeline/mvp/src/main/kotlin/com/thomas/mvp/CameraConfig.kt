package com.thomas.mvp

import org.yaml.snakeyaml.LoaderOptions
import org.yaml.snakeyaml.Yaml
import org.yaml.snakeyaml.constructor.SafeConstructor
import java.nio.file.Files
import java.nio.file.Path
import kotlin.collections.get
import kotlin.io.path.exists

data class PeakPoint(
    val hour: Double,
    val speedKmh: Double,
    val occupancy: Double,
)

data class SpeedProfile(
    val freeFlowKmh: Double,
    val freeFlowOccupancy: Double,
    val peaks: List<PeakPoint>,
)

sealed interface CameraPlacement {
    data class ExplicitArc(val arcId: Int) : CameraPlacement
    data class Coordinate(
        val lat: Double,
        val lon: Double,
        val flowBearingDeg: Double,
    ) : CameraPlacement
}

data class CameraSpec(
    val id: Int,
    val label: String,
    val profileName: String,
    val placement: CameraPlacement,
)

data class CameraConfig(
    val profiles: Map<String, SpeedProfile>,
    val cameras: List<CameraSpec>,
) {
    companion object {
        fun empty(): CameraConfig = CameraConfig(emptyMap(), emptyList())
    }
}

object CameraConfigLoader {
    fun load(path: Path): CameraConfig {
        require(path.exists()) { "Camera config file does not exist: $path" }

        val loaderOptions = LoaderOptions().apply {
            isAllowDuplicateKeys = false
            isWarnOnDuplicateKeys = false
        }
        val yaml = Yaml(SafeConstructor(loaderOptions))
        val root = Files.newInputStream(path).use { input ->
            yaml.load<Any?>(input)
        }

        if (root == null) {
            return CameraConfig.empty()
        }

        val rootMap = root.asMap("root YAML document")
        val profiles = parseProfiles(rootMap["profiles"])
        val cameras = parseCameras(rootMap["cameras"])
        val seenCameraIds = HashSet<Int>(cameras.size)

        for (camera in cameras) {
            require(seenCameraIds.add(camera.id)) {
                "Duplicate camera id ${camera.id} ('${camera.label}') in cameras.yaml"
            }
        }

        for (camera in cameras) {
            require(profiles.containsKey(camera.profileName)) {
                "Camera ${camera.id} ('${camera.label}') references unknown profile '${camera.profileName}'"
            }
        }

        return CameraConfig(profiles = profiles, cameras = cameras)
    }

    private fun parseProfiles(node: Any?): Map<String, SpeedProfile> {
        if (node == null) {
            return emptyMap()
        }

        val profilesMap = node.asMap("profiles")
        return profilesMap.entries.associate { (rawName, rawProfile) ->
            val profileName = rawName.asString("profile name")
            require(profileName.isNotBlank()) { "profile name must not be blank" }
            val profileMap = rawProfile.asMap("profile '$profileName'")
            profileName to parseProfile(profileName, profileMap)
        }
    }

    private fun parseProfile(profileName: String, profileMap: Map<*, *>): SpeedProfile {
        val freeFlowKmh = profileMap.requiredDouble("free_flow_kmh", "profile '$profileName'")
        require(freeFlowKmh > 0.0) {
            "profile '$profileName' free_flow_kmh must be > 0, got $freeFlowKmh"
        }

        val freeFlowOccupancy =
            profileMap.requiredDouble("free_flow_occupancy", "profile '$profileName'")
        require(freeFlowOccupancy in 0.0..1.0) {
            "profile '$profileName' free_flow_occupancy must be in [0,1], got $freeFlowOccupancy"
        }

        val peaks = profileMap.optionalList("peaks", "profile '$profileName'")?.mapIndexed { idx, rawPeak ->
            val peakMap = rawPeak.asMap("profile '$profileName' peak[$idx]")
            val hour = normalizeHour(peakMap.requiredDouble("hour", "profile '$profileName' peak[$idx]"))
            val speedKmh = peakMap.requiredDouble("speed_kmh", "profile '$profileName' peak[$idx]")
            val occupancy = peakMap.requiredDouble("occupancy", "profile '$profileName' peak[$idx]")

            require(speedKmh > 0.0) {
                "profile '$profileName' peak[$idx] speed_kmh must be > 0, got $speedKmh"
            }
            require(occupancy in 0.0..1.0) {
                "profile '$profileName' peak[$idx] occupancy must be in [0,1], got $occupancy"
            }

            PeakPoint(hour = hour, speedKmh = speedKmh, occupancy = occupancy)
        }.orEmpty()

        return SpeedProfile(
            freeFlowKmh = freeFlowKmh,
            freeFlowOccupancy = freeFlowOccupancy,
            peaks = peaks,
        )
    }

    private fun parseCameras(node: Any?): List<CameraSpec> {
        if (node == null) {
            return emptyList()
        }

        val cameras = node.asList("cameras")
        return cameras.mapIndexed { idx, rawCamera ->
            parseCamera(idx, rawCamera.asMap("camera[$idx]"))
        }
    }

    private fun parseCamera(index: Int, cameraMap: Map<*, *>): CameraSpec {
        val id = cameraMap.requiredInt("id", "camera[$index]")
        val label = cameraMap.requiredString("label", "camera[$index]")
        val profileName = cameraMap.requiredString("profile", "camera[$index]")
        require(label.isNotBlank()) { "camera[$index].label must not be blank" }
        require(profileName.isNotBlank()) { "camera[$index].profile must not be blank" }

        val arcId = cameraMap.optionalInt("arc_id", "camera[$index]")
        val lat = cameraMap.optionalDouble("lat", "camera[$index]")
        val lon = cameraMap.optionalDouble("lon", "camera[$index]")
        val flowBearingDeg = cameraMap.optionalDouble("flow_bearing_deg", "camera[$index]")

        val hasExplicitArc = arcId != null
        val hasCoordinateMode = lat != null || lon != null || flowBearingDeg != null

        require(hasExplicitArc xor hasCoordinateMode) {
            "camera[$index] must provide exactly one placement mode: either arc_id or lat/lon/flow_bearing_deg"
        }

        val placement = if (hasExplicitArc) {
            require(arcId >= 0) { "camera[$index] arc_id must be >= 0, got $arcId" }
            CameraPlacement.ExplicitArc(arcId)
        } else {
            require(lat != null && lon != null && flowBearingDeg != null) {
                "camera[$index] coordinate mode requires lat, lon, and flow_bearing_deg"
            }
            require(lat in -90.0..90.0) { "camera[$index] lat must be in [-90,90], got $lat" }
            require(lon in -180.0..180.0) { "camera[$index] lon must be in [-180,180], got $lon" }

            CameraPlacement.Coordinate(
                lat = lat,
                lon = lon,
                flowBearingDeg = normalizeBearing(flowBearingDeg),
            )
        }

        return CameraSpec(
            id = id,
            label = label,
            profileName = profileName,
            placement = placement,
        )
    }

    private fun normalizeHour(hour: Double): Double {
        require(hour.isFinite()) { "hour must be finite, got $hour" }
        val normalized = hour % 24.0
        return if (normalized < 0.0) normalized + 24.0 else normalized
    }

    private fun normalizeBearing(bearingDeg: Double): Double {
        require(bearingDeg.isFinite()) { "flow_bearing_deg must be finite, got $bearingDeg" }
        val normalized = bearingDeg % 360.0
        return if (normalized < 0.0) normalized + 360.0 else normalized
    }
}

private fun Any?.asMap(context: String): Map<*, *> {
    require(this is Map<*, *>) { "$context must be a mapping, got ${this?.javaClass?.name ?: "null"}" }
    return this
}

private fun Any?.asList(context: String): List<*> {
    require(this is List<*>) { "$context must be a list, got ${this?.javaClass?.name ?: "null"}" }
    return this
}

private fun Any?.asString(context: String): String {
    require(this is String) { "$context must be a string, got ${this?.javaClass?.name ?: "null"}" }
    return this
}

private fun Map<*, *>.requiredString(key: String, context: String): String =
    (this[key] ?: error("$context is missing required key '$key'")).asString("$context.$key")

private fun Map<*, *>.requiredDouble(key: String, context: String): Double {
    val raw = this[key] ?: error("$context is missing required key '$key'")
    return when (raw) {
        is Number -> raw.toDouble()
        is String -> raw.toDoubleOrNull()
            ?: error("$context.$key must be numeric, got '$raw'")
        else -> error("$context.$key must be numeric, got ${raw.javaClass.name}")
    }.also {
        require(it.isFinite()) { "$context.$key must be finite, got $it" }
    }
}

private fun Map<*, *>.requiredInt(key: String, context: String): Int {
    val raw = this[key] ?: error("$context is missing required key '$key'")
    return when (raw) {
        is Int -> raw
        is Long -> {
            require(raw in Int.MIN_VALUE.toLong()..Int.MAX_VALUE.toLong()) {
                "$context.$key = $raw exceeds Int range"
            }
            raw.toInt()
        }
        is Number -> {
            val doubleValue = raw.toDouble()
            require(doubleValue % 1.0 == 0.0) { "$context.$key must be an integer, got $raw" }
            require(doubleValue in Int.MIN_VALUE.toDouble()..Int.MAX_VALUE.toDouble()) {
                "$context.$key = $raw exceeds Int range"
            }
            doubleValue.toInt()
        }
        is String -> raw.toIntOrNull()
            ?: error("$context.$key must be an integer, got '$raw'")
        else -> error("$context.$key must be an integer, got ${raw.javaClass.name}")
    }
}

private fun Map<*, *>.optionalDouble(key: String, context: String): Double? {
    val raw = this[key] ?: return null
    return when (raw) {
        is Number -> raw.toDouble()
        is String -> raw.toDoubleOrNull()
            ?: error("$context.$key must be numeric, got '$raw'")
        else -> error("$context.$key must be numeric, got ${raw.javaClass.name}")
    }.also {
        require(it.isFinite()) { "$context.$key must be finite, got $it" }
    }
}

private fun Map<*, *>.optionalInt(key: String, context: String): Int? {
    val raw = this[key] ?: return null
    return when (raw) {
        is Int -> raw
        is Long -> {
            require(raw in Int.MIN_VALUE.toLong()..Int.MAX_VALUE.toLong()) {
                "$context.$key = $raw exceeds Int range"
            }
            raw.toInt()
        }
        is Number -> {
            val doubleValue = raw.toDouble()
            require(doubleValue % 1.0 == 0.0) { "$context.$key must be an integer, got $raw" }
            require(doubleValue in Int.MIN_VALUE.toDouble()..Int.MAX_VALUE.toDouble()) {
                "$context.$key = $raw exceeds Int range"
            }
            doubleValue.toInt()
        }
        is String -> raw.toIntOrNull()
            ?: error("$context.$key must be an integer, got '$raw'")
        else -> error("$context.$key must be an integer, got ${raw.javaClass.name}")
    }
}

private fun Map<*, *>.optionalList(key: String, context: String): List<*>? {
    val raw = this[key] ?: return null
    return raw.asList("$context.$key")
}
