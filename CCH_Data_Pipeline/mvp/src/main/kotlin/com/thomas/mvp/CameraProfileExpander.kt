package com.thomas.mvp

import org.slf4j.LoggerFactory
import java.util.Locale
import kotlin.collections.iterator

class CameraProfileExpander(
    private val roadIndex: RoadIndex,
    private val arcManifest: ArcManifest,
    private val wayIndex: IntArray,
) {
    init {
        require(wayIndex.size == arcManifest.size) {
            "wayIndex length (${wayIndex.size}) must match arc manifest size (${arcManifest.size})"
        }
        require(roadIndex.firstArcOffsetByWay.isNotEmpty()) {
            "roadIndex.firstArcOffsetByWay must not be empty"
        }
        require(roadIndex.firstArcOffsetByWay.last() == roadIndex.arcIdsByWay.size) {
            "roadIndex sentinel (${roadIndex.firstArcOffsetByWay.last()}) must match arcIdsByWay length (${roadIndex.arcIdsByWay.size})"
        }
        for (wayId in 0 until roadIndex.wayCount) {
            require(roadIndex.firstArcOffsetByWay[wayId] <= roadIndex.firstArcOffsetByWay[wayId + 1]) {
                "roadIndex.firstArcOffsetByWay must be non-decreasing; found ${roadIndex.firstArcOffsetByWay[wayId]} > ${roadIndex.firstArcOffsetByWay[wayId + 1]} at wayId=$wayId"
            }
        }
        for ((offset, arcId) in roadIndex.arcIdsByWay.withIndex()) {
            require(arcId in wayIndex.indices) {
                "roadIndex.arcIdsByWay[$offset] = $arcId is outside base-arc range 0..${wayIndex.size - 1}"
            }
        }
    }

    fun expand(anchorProfiles: Map<Int, SpeedProfile>): Map<Int, SpeedProfile> {
        if (anchorProfiles.isEmpty()) {
            return emptyMap()
        }

        val expanded = LinkedHashMap<Int, SpeedProfile>()

        for ((anchorArcId, profile) in anchorProfiles) {
            require(anchorArcId in wayIndex.indices) {
                "anchor arc_id $anchorArcId is outside base-arc range 0..${wayIndex.size - 1}"
            }

            val wayId = wayIndex[anchorArcId]
            require(wayId in 0 until roadIndex.wayCount) {
                "anchor arc_id $anchorArcId maps to invalid routingWayId $wayId; expected 0..${roadIndex.wayCount - 1}"
            }
            require(arcManifest.routingWayId(anchorArcId) == wayId) {
                "anchor arc_id $anchorArcId has wayIndex=$wayId but arc manifest routingWayId=${arcManifest.routingWayId(anchorArcId)}"
            }

            val anchorAntiparallel = arcManifest.isAntiparallelToWay(anchorArcId)
            val anchorBearing = arcManifest.bearingDeg(anchorArcId)
            val roadName = roadIndex.nameByWay[wayId].ifBlank { "<unnamed road>" }

            if (expanded.containsKey(anchorArcId)) {
                logger.warn(
                    "anchor arc {} on way {} ('{}') is already covered by earlier propagation; first-camera-wins will shadow this anchor",
                    anchorArcId,
                    wayId,
                    roadName,
                )
            }

            val startOffset = roadIndex.firstArcOffsetByWay[wayId]
            val endOffset = roadIndex.firstArcOffsetByWay[wayId + 1]
            var propagatedArcCount = 0
            var bearingWarningCount = 0

            for (offset in startOffset until endOffset) {
                val siblingArcId = roadIndex.arcIdsByWay[offset]
                require(wayIndex[siblingArcId] == wayId) {
                    "roadIndex group for wayId=$wayId contains arc $siblingArcId whose wayIndex is ${wayIndex[siblingArcId]}"
                }
                require(arcManifest.routingWayId(siblingArcId) == wayId) {
                    "roadIndex group for wayId=$wayId contains arc $siblingArcId whose arc manifest routingWayId is ${arcManifest.routingWayId(siblingArcId)}"
                }

                if (arcManifest.isAntiparallelToWay(siblingArcId) != anchorAntiparallel) {
                    continue
                }

                val bearingDiff = CameraResolver.circularAngleDiff(anchorBearing, arcManifest.bearingDeg(siblingArcId))
                if (bearingDiff > BEARING_WARN_THRESHOLD_DEG) {
                    bearingWarningCount++
                    logger.warn(
                        "arc {} on way {} ('{}') has bearing diff {}deg from anchor arc {} (> {}deg threshold); propagating anyway",
                        siblingArcId,
                        wayId,
                        roadName,
                        String.format(Locale.ROOT, "%.1f", bearingDiff),
                        anchorArcId,
                        String.format(Locale.ROOT, "%.1f", BEARING_WARN_THRESHOLD_DEG),
                    )
                }

                val existing = expanded.putIfAbsent(siblingArcId, profile)
                if (existing == null) {
                    propagatedArcCount++
                } else if (siblingArcId != anchorArcId) {
                    logger.warn(
                        "arc {} on way {} ('{}') is already covered by earlier propagation; skipping propagation from anchor arc {}",
                        siblingArcId,
                        wayId,
                        roadName,
                        anchorArcId,
                    )
                }
            }

            logger.info(
                "camera propagation: anchorArc={} way={} road='{}' antiparallel={} propagatedArcs={} bearingWarnings={}",
                anchorArcId,
                wayId,
                roadName,
                anchorAntiparallel,
                propagatedArcCount,
                bearingWarningCount,
            )
        }

        return expanded
    }

    companion object {
        private val logger = LoggerFactory.getLogger(CameraProfileExpander::class.java)
        private const val BEARING_WARN_THRESHOLD_DEG = 90.0
    }
}
