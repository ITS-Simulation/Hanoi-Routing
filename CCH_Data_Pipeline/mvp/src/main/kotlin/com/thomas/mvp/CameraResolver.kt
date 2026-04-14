package com.thomas.mvp

import org.slf4j.LoggerFactory
import java.util.Locale
import java.util.PriorityQueue
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.hypot
import kotlin.math.min

data class ResolvedCamera(
    val camera: CameraSpec,
    val arcId: Int,
    val roadName: String,
    val bearingDeg: Double,
    val distanceMeters: Double,
    val bearingDiffDeg: Double,
)

class CameraResolver(
    private val arcManifest: ArcManifest,
    private val originalEdgeCount: Int,
) {
    init {
        require(arcManifest.size == originalEdgeCount) {
            "arc manifest size (${arcManifest.size}) must match original edge count ($originalEdgeCount)"
        }
    }

    fun resolveAll(cameras: List<CameraSpec>): List<ResolvedCamera> {
        return cameras.map(::resolve)
    }

    fun resolve(camera: CameraSpec): ResolvedCamera {
        return when (val placement = camera.placement) {
            is CameraPlacement.ExplicitArc -> resolveExplicitArc(camera, placement)
            is CameraPlacement.Coordinate -> resolveCoordinate(camera, placement)
        }
    }

    private fun resolveExplicitArc(
        camera: CameraSpec,
        placement: CameraPlacement.ExplicitArc,
    ): ResolvedCamera {
        require(placement.arcId in 0 until originalEdgeCount) {
            "Camera ${camera.id} ('${camera.label}') has arc_id=${placement.arcId}, " +
                "which is outside original/base arc range 0..${originalEdgeCount - 1}"
        }

        val metadata = arcManifest.metadata(placement.arcId)
        logger.info(
            "camera '{}' uses explicit arc_id={} (road='{}', bearing={}deg)",
            camera.label,
            placement.arcId,
            metadata.name,
            metadata.bearingDeg,
        )
        return ResolvedCamera(
            camera = camera,
            arcId = placement.arcId,
            roadName = metadata.name,
            bearingDeg = metadata.bearingDeg,
            distanceMeters = 0.0,
            bearingDiffDeg = 0.0,
        )
    }

    private fun resolveCoordinate(
        camera: CameraSpec,
        placement: CameraPlacement.Coordinate,
    ): ResolvedCamera {
        val nearby = findNearbyBaseArcs(placement.lat, placement.lon)
        val scored = nearby
            .map { candidate ->
                val metadata = arcManifest.metadata(candidate.arcId)
                val bearingDiffDeg = circularAngleDiff(placement.flowBearingDeg, metadata.bearingDeg)
                ScoredCandidate(
                    arcId = candidate.arcId,
                    roadName = metadata.name,
                    distanceMeters = candidate.distanceMeters,
                    bearingDeg = metadata.bearingDeg,
                    bearingDiffDeg = bearingDiffDeg,
                    score = candidate.distanceMeters + 2.0 * bearingDiffDeg,
                )
            }
            .filter { it.bearingDiffDeg <= MAX_BEARING_DIFF_DEGREES }
            .sortedBy { it.score }

        val best = scored.firstOrNull() ?: error(
            buildString {
                append(
                    "No heading-consistent arc found for camera ${camera.id} ('${camera.label}') " +
                        "at (${placement.lat}, ${placement.lon}) with flow_bearing_deg=${placement.flowBearingDeg}. "
                )
                append("Nearest candidates were: ")
                append(
                    nearby
                        .sortedBy { it.distanceMeters }
                        .take(5)
                        .joinToString("; ") { candidate ->
                            val metadata = arcManifest.metadata(candidate.arcId)
                            val diff = circularAngleDiff(placement.flowBearingDeg, metadata.bearingDeg)
                            "arc=${candidate.arcId}, road='${metadata.name}', distance=${"%.1f".format(Locale.ROOT, candidate.distanceMeters)}m, " +
                                "bearing=${"%.1f".format(Locale.ROOT, metadata.bearingDeg)}deg, diff=${"%.1f".format(Locale.ROOT, diff)}deg"
                        }
                )
            }
        )

        logger.info(
            "camera '{}' resolved to arc_id={} (road='{}', distance={}m, arc_bearing={}deg, flow_bearing={}deg, diff={}deg)",
            camera.label,
            best.arcId,
            best.roadName,
            String.format(Locale.ROOT, "%.2f", best.distanceMeters),
            String.format(Locale.ROOT, "%.2f", best.bearingDeg),
            String.format(Locale.ROOT, "%.2f", placement.flowBearingDeg),
            String.format(Locale.ROOT, "%.2f", best.bearingDiffDeg),
        )

        return ResolvedCamera(
            camera = camera,
            arcId = best.arcId,
            roadName = best.roadName,
            bearingDeg = best.bearingDeg,
            distanceMeters = best.distanceMeters,
            bearingDiffDeg = best.bearingDiffDeg,
        )
    }

    private fun findNearbyBaseArcs(lat: Double, lon: Double): List<DistanceCandidate> {
        val queue = PriorityQueue<DistanceCandidate>(compareByDescending { it.distanceMeters })
        for (arcId in 0 until originalEdgeCount) {
            val distanceMeters = pointToSegmentDistanceMeters(
                pointLat = lat,
                pointLon = lon,
                tailLat = arcManifest.tailLat(arcId),
                tailLon = arcManifest.tailLon(arcId),
                headLat = arcManifest.headLat(arcId),
                headLon = arcManifest.headLon(arcId),
            )

            if (queue.size < MAX_DISTANCE_CANDIDATES) {
                queue.add(DistanceCandidate(arcId = arcId, distanceMeters = distanceMeters))
            } else if (distanceMeters < queue.peek().distanceMeters) {
                queue.poll()
                queue.add(DistanceCandidate(arcId = arcId, distanceMeters = distanceMeters))
            }
        }

        return queue.toList()
    }

    private fun pointToSegmentDistanceMeters(
        pointLat: Double,
        pointLon: Double,
        tailLat: Double,
        tailLon: Double,
        headLat: Double,
        headLon: Double,
    ): Double {
        val refLatRad = Math.toRadians(pointLat)
        val earthRadiusM = 6_371_000.0

        fun project(lat: Double, lon: Double): Pair<Double, Double> {
            val x = Math.toRadians(lon - pointLon) * earthRadiusM * cos(refLatRad)
            val y = Math.toRadians(lat - pointLat) * earthRadiusM
            return x to y
        }

        val (ax, ay) = project(tailLat, tailLon)
        val (bx, by) = project(headLat, headLon)
        val abx = bx - ax
        val aby = by - ay
        val abLenSq = abx * abx + aby * aby

        if (abLenSq == 0.0) {
            return hypot(ax, ay)
        }

        val t = ((-ax) * abx + (-ay) * aby) / abLenSq
        val clampedT = t.coerceIn(0.0, 1.0)
        val closestX = ax + clampedT * abx
        val closestY = ay + clampedT * aby
        return hypot(closestX, closestY)
    }

    companion object {
        private val logger = LoggerFactory.getLogger(CameraResolver::class.java)

        private const val MAX_DISTANCE_CANDIDATES = 128
        private const val MAX_BEARING_DIFF_DEGREES = 60.0

        fun circularAngleDiff(aDeg: Double, bDeg: Double): Double {
            val raw = abs(normalizeBearing(aDeg) - normalizeBearing(bDeg))
            return min(raw, 360.0 - raw)
        }

        private fun normalizeBearing(bearingDeg: Double): Double {
            val normalized = bearingDeg % 360.0
            return if (normalized < 0.0) normalized + 360.0 else normalized
        }
    }

    private data class DistanceCandidate(
        val arcId: Int,
        val distanceMeters: Double,
    )

    private data class ScoredCandidate(
        val arcId: Int,
        val roadName: String,
        val distanceMeters: Double,
        val bearingDeg: Double,
        val bearingDiffDeg: Double,
        val score: Double,
    )
}
