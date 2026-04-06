package com.thomas.cch_app

import org.slf4j.LoggerFactory
import kotlin.math.exp
import kotlin.math.min
import kotlin.math.pow
import kotlin.math.sin
import kotlin.math.PI

class WeightGenerator(
    private val inputs: GraphInputs,
    private val fanOut: LineGraphFanOut,
    private val cameraProfiles: Map<Int, SpeedProfile>,
    private val closedEdges: Set<Int> = emptySet(),
) {
    init {
        require(fanOut.originalEdgeCount == inputs.originalEdgeCount) {
            "fan-out original edge count (${fanOut.originalEdgeCount}) must match graph original edge count (${inputs.originalEdgeCount})"
        }
        require(fanOut.lineGraphEdgeCount == inputs.lineGraphEdgeCount) {
            "fan-out LG edge count (${fanOut.lineGraphEdgeCount}) must match graph LG edge count (${inputs.lineGraphEdgeCount})"
        }
        for (originalEdgeId in cameraProfiles.keys) {
            require(originalEdgeId in 0 until inputs.originalEdgeCount) {
                "camera profile is assigned to invalid original edge $originalEdgeId; expected 0..${inputs.originalEdgeCount - 1}"
            }
        }
        for (originalEdgeId in closedEdges) {
            require(originalEdgeId in 0 until inputs.originalEdgeCount) {
                "closedEdges contains invalid arc_id $originalEdgeId; expected 0..${inputs.originalEdgeCount - 1}"
            }
        }
    }

    fun generateWeights(hour: Double): IntArray {
        val normalizedHour = normalizeHour(hour)
        val todRaw = timeOfDayFactor(normalizedHour)
        val lgWeights = IntArray(inputs.lineGraphEdgeCount)

        for (originalEdgeId in 0 until inputs.originalEdgeCount) {
            if (originalEdgeId in closedEdges) {
                for (offset in fanOut.incomingLgEdgeRange(originalEdgeId)) {
                    lgWeights[fanOut.lgEdgeIdsByOriginalTarget[offset]] = ROAD_CLOSED
                }
                continue
            }

            val targetEdgeWeight = cameraProfiles[originalEdgeId]?.let { profile ->
                val (speedKmh, occupancy) = profileSpeed(profile, normalizedHour)
                val baseTravelTimeMs = inputs.geoDistance[originalEdgeId] * 3600.0 / speedKmh
                clampWeight(baseTravelTimeMs * occupancyFactor(occupancy))
            } ?: run {
                val scale = highwayCongestionScale(inputs.edgeHighway[originalEdgeId])
                val factor = 1.0 + (todRaw - 1.0) * scale
                clampWeight(inputs.originalTravelTime[originalEdgeId] * factor)
            }

            for (offset in fanOut.incomingLgEdgeRange(originalEdgeId)) {
                val lgEdgeId = fanOut.lgEdgeIdsByOriginalTarget[offset]
                lgWeights[lgEdgeId] = safeAdd(targetEdgeWeight, fanOut.turnCostByLgEdge[lgEdgeId])
            }
        }

        return lgWeights
    }

    fun logSummary(hour: Double, weights: IntArray) {
        val minWeight = weights.filter { it != ROAD_CLOSED }.minOrNull() ?: 0
        val maxWeight = weights.filter { it != ROAD_CLOSED }.maxOrNull() ?: 0
        val closedLgEdges = weights.count { it == ROAD_CLOSED }
        logger.info(
            "generated weight vector: hour={} normalizedHour={} cameraCoveredEdges={} closedOriginalEdges={} closedLgEdges={} minWeight={} maxWeight={} numLgEdges={}",
            hour,
            normalizeHour(hour),
            cameraProfiles.size,
            closedEdges.size,
            closedLgEdges,
            minWeight,
            maxWeight,
            weights.size,
        )
    }

    companion object {
        private val logger = LoggerFactory.getLogger(WeightGenerator::class.java)
        const val MAX_ALLOWED_WEIGHT: Int = 2_147_483_646

        /** Sentinel value for a temporarily closed road (= CCH INFINITY = u32::MAX / 2).
         *  The hanoi-server accepts this value and the CCH treats any path through it
         *  as unreachable, because INFINITY + any_weight >= INFINITY in u32 arithmetic. */
        const val ROAD_CLOSED: Int = 2_147_483_647

        fun gaussian(hour: Double, center: Double, sigma: Double = 1.2): Double {
            require(sigma > 0.0) { "sigma must be > 0, got $sigma" }
            val delta = circularHourDiff(normalizeHour(hour), normalizeHour(center))
            return exp(-0.5 * (delta / sigma).pow(2))
        }

        fun lerp(a: Double, b: Double, t: Double): Double = a + (b - a) * t

        fun profileSpeed(profile: SpeedProfile, hour: Double): Pair<Double, Double> {
            val normalizedHour = normalizeHour(hour)
            var speed = profile.freeFlowKmh
            var occupancy = profile.freeFlowOccupancy
            var strongestPeakWeight = 0.0

            for (peak in profile.peaks) {
                val weight = gaussian(normalizedHour, center = peak.hour)
                strongestPeakWeight = maxOf(strongestPeakWeight, weight)
                speed = lerp(speed, peak.speedKmh, weight)
                occupancy = lerp(occupancy, peak.occupancy, weight)
            }

            val freeFlowVarianceWeight = (1.0 - strongestPeakWeight).coerceIn(0.0, 1.0)
            if (freeFlowVarianceWeight > 0.0) {
                val speedJitter = freeFlowSpeedJitter(normalizedHour) * freeFlowVarianceWeight
                val occupancyJitter = freeFlowOccupancyJitter(normalizedHour) * freeFlowVarianceWeight
                speed *= 1.0 + speedJitter
                occupancy = (occupancy + occupancyJitter).coerceIn(0.0, 1.0)
            }

            return speed to occupancy
        }

        fun occupancyFactor(occupancy: Double): Double {
            require(occupancy.isFinite()) { "occupancy must be finite, got $occupancy" }
            return 1.0 + 0.5 * (occupancy - 0.2)
        }

        fun timeOfDayFactor(hour: Double): Double {
            val normalizedHour = normalizeHour(hour)
            val morning = 0.35 * gaussian(normalizedHour, center = 7.5, sigma = 1.2)
            val evening = 0.45 * gaussian(normalizedHour, center = 17.5, sigma = 1.5)
            val night = -0.15 * gaussian(normalizedHour, center = 2.0, sigma = 2.0)
            return 1.0 + morning + evening + night
        }

        /**
         * Small deterministic free-flow wobble so off-peak weights are not perfectly flat.
         * This is intentionally bounded and fades out near configured peaks.
         */
        fun freeFlowSpeedJitter(hour: Double): Double {
            val theta = normalizeHour(hour) / 24.0 * 2.0 * PI
            return 0.012 * sin(theta * 3.0 + 0.4) + 0.006 * sin(theta * 7.0 - 1.1)
        }

        fun freeFlowOccupancyJitter(hour: Double): Double {
            val theta = normalizeHour(hour) / 24.0 * 2.0 * PI
            return 0.012 * sin(theta * 2.0 - 0.8) + 0.004 * sin(theta * 5.0 + 0.6)
        }

        fun highwayCongestionScale(highway: String): Double = when (highway) {
            "motorway", "motorway_link" -> 1.00
            "trunk", "trunk_link" -> 0.80
            "primary", "primary_link" -> 0.85
            "secondary", "secondary_link" -> 0.55
            "tertiary", "tertiary_link" -> 0.35
            "residential", "living_street" -> 0.35
            "service" -> 0.15
            else -> 0.30
        }

        fun normalizeHour(hour: Double): Double {
            require(hour.isFinite()) { "hour must be finite, got $hour" }
            val normalized = hour % 24.0
            return if (normalized < 0.0) normalized + 24.0 else normalized
        }

        fun circularHourDiff(hour: Double, center: Double): Double {
            val raw = kotlin.math.abs(normalizeHour(hour) - normalizeHour(center))
            return min(raw, 24.0 - raw)
        }

        private fun safeAdd(a: Int, b: Int): Int {
            require(a in 1..MAX_ALLOWED_WEIGHT) { "weight $a is outside allowed range 1..$MAX_ALLOWED_WEIGHT" }
            require(b >= 0) { "turn cost must be >= 0, got $b" }
            val sum = a.toLong() + b.toLong()
            return if (sum > MAX_ALLOWED_WEIGHT.toLong()) {
                MAX_ALLOWED_WEIGHT
            } else {
                sum.toInt()
            }
        }

        private fun clampWeight(rawWeight: Double): Int {
            require(rawWeight.isFinite()) { "weight must be finite, got $rawWeight" }
            return rawWeight.toInt().coerceIn(1, MAX_ALLOWED_WEIGHT)
        }
    }
}
