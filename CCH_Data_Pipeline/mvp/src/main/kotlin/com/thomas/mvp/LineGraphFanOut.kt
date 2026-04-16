package com.thomas.mvp

import org.slf4j.LoggerFactory

data class LineGraphFanOut(
    val firstLgEdgeOffsetByOriginalTarget: IntArray,
    val lgEdgeIdsByOriginalTarget: IntArray,
    val turnCostByLgEdge: IntArray,
    val normalizedTargetOriginalByLgNode: IntArray,
) {
    val originalEdgeCount: Int
        get() = firstLgEdgeOffsetByOriginalTarget.size - 1

    val lineGraphEdgeCount: Int
        get() = lgEdgeIdsByOriginalTarget.size

    fun incomingLgEdgeRange(originalEdgeId: Int): IntRange {
        require(originalEdgeId in 0 until originalEdgeCount) {
            "original edge $originalEdgeId is outside reverse-index range 0..${originalEdgeCount - 1}"
        }
        return firstLgEdgeOffsetByOriginalTarget[originalEdgeId] until
            firstLgEdgeOffsetByOriginalTarget[originalEdgeId + 1]
    }

    companion object {
        private val logger = LoggerFactory.getLogger(LineGraphFanOut::class.java)

        fun build(
            originalEdgeCount: Int,
            lineGraphNodeCount: Int,
            lineGraphHead: IntArray,
            lineGraphBaselineWeights: IntArray,
            originalTravelTime: IntArray,
            splitMap: IntArray,
        ): LineGraphFanOut {
            require(originalEdgeCount == originalTravelTime.size) {
                "original edge count $originalEdgeCount does not match original travel_time length ${originalTravelTime.size}"
            }
            require(lineGraphBaselineWeights.size == lineGraphHead.size) {
                "line graph travel_time length (${lineGraphBaselineWeights.size}) must match line graph head length (${lineGraphHead.size})"
            }
            require(lineGraphNodeCount == originalEdgeCount + splitMap.size) {
                "line graph node count ($lineGraphNodeCount) must equal original edge count ($originalEdgeCount) + split node count (${splitMap.size})"
            }
            for ((idx, originalEdgeId) in splitMap.withIndex()) {
                require(originalEdgeId in 0 until originalEdgeCount) {
                    "via_way_split_map[$idx] = $originalEdgeId is outside original edge range 0..${originalEdgeCount - 1}"
                }
            }

            val normalizedTargetOriginalByLgNode = IntArray(lineGraphNodeCount)
            for (nodeId in 0 until lineGraphNodeCount) {
                normalizedTargetOriginalByLgNode[nodeId] =
                    if (nodeId < originalEdgeCount) {
                        nodeId
                    } else {
                        splitMap[nodeId - originalEdgeCount]
                    }
            }

            val counts = IntArray(originalEdgeCount)
            val turnCostByLgEdge = IntArray(lineGraphHead.size)

            for (lgEdgeId in lineGraphHead.indices) {
                val targetLgNode = lineGraphHead[lgEdgeId]
                require(targetLgNode in 0 until lineGraphNodeCount) {
                    "line_graph/head[$lgEdgeId] = $targetLgNode is outside node range 0..${lineGraphNodeCount - 1}"
                }

                val targetOriginalEdge = normalizedTargetOriginalByLgNode[targetLgNode]
                counts[targetOriginalEdge]++

                val baselineWeight = lineGraphBaselineWeights[lgEdgeId]
                val baseTargetTravelTime = originalTravelTime[targetOriginalEdge]
                require(baselineWeight >= baseTargetTravelTime) {
                    "line_graph/travel_time[$lgEdgeId] = $baselineWeight is smaller than " +
                        "the target original edge travel_time[$targetOriginalEdge] = $baseTargetTravelTime"
                }
                turnCostByLgEdge[lgEdgeId] = baselineWeight - baseTargetTravelTime
            }

            val firstLgEdgeOffsetByOriginalTarget = IntArray(originalEdgeCount + 1)
            for (originalEdgeId in 0 until originalEdgeCount) {
                firstLgEdgeOffsetByOriginalTarget[originalEdgeId + 1] =
                    firstLgEdgeOffsetByOriginalTarget[originalEdgeId] + counts[originalEdgeId]
            }

            val lgEdgeIdsByOriginalTarget = IntArray(lineGraphHead.size)
            val cursor = firstLgEdgeOffsetByOriginalTarget.copyOf()

            for (lgEdgeId in lineGraphHead.indices) {
                val targetLgNode = lineGraphHead[lgEdgeId]
                val targetOriginalEdge = normalizedTargetOriginalByLgNode[targetLgNode]
                val slot = cursor[targetOriginalEdge]
                lgEdgeIdsByOriginalTarget[slot] = lgEdgeId
                cursor[targetOriginalEdge] = slot + 1
            }
            for (originalEdgeId in 0 until originalEdgeCount) {
                require(cursor[originalEdgeId] == firstLgEdgeOffsetByOriginalTarget[originalEdgeId + 1]) {
                    "reverse index fill mismatch for original edge $originalEdgeId: " +
                        "cursor=${cursor[originalEdgeId]}, expected=${firstLgEdgeOffsetByOriginalTarget[originalEdgeId + 1]}"
                }
            }

            require(firstLgEdgeOffsetByOriginalTarget.last() == lineGraphHead.size) {
                "reverse index sentinel (${firstLgEdgeOffsetByOriginalTarget.last()}) must match line graph edge count (${lineGraphHead.size})"
            }

            logger.info(
                "built reverse index: originalEdges={}, lineGraphNodes={}, lineGraphEdges={}, splitNodes={}",
                originalEdgeCount,
                lineGraphNodeCount,
                lineGraphHead.size,
                splitMap.size,
            )

            return LineGraphFanOut(
                firstLgEdgeOffsetByOriginalTarget = firstLgEdgeOffsetByOriginalTarget,
                lgEdgeIdsByOriginalTarget = lgEdgeIdsByOriginalTarget,
                turnCostByLgEdge = turnCostByLgEdge,
                normalizedTargetOriginalByLgNode = normalizedTargetOriginalByLgNode,
            )
        }
    }

    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (javaClass != other?.javaClass) return false

        other as LineGraphFanOut

        if (!firstLgEdgeOffsetByOriginalTarget.contentEquals(other.firstLgEdgeOffsetByOriginalTarget)) return false
        if (!lgEdgeIdsByOriginalTarget.contentEquals(other.lgEdgeIdsByOriginalTarget)) return false
        if (!turnCostByLgEdge.contentEquals(other.turnCostByLgEdge)) return false
        if (!normalizedTargetOriginalByLgNode.contentEquals(other.normalizedTargetOriginalByLgNode)) return false
        if (originalEdgeCount != other.originalEdgeCount) return false
        if (lineGraphEdgeCount != other.lineGraphEdgeCount) return false

        return true
    }

    override fun hashCode(): Int {
        var result = firstLgEdgeOffsetByOriginalTarget.contentHashCode()
        result = 31 * result + lgEdgeIdsByOriginalTarget.contentHashCode()
        result = 31 * result + turnCostByLgEdge.contentHashCode()
        result = 31 * result + normalizedTargetOriginalByLgNode.contentHashCode()
        result = 31 * result + originalEdgeCount
        result = 31 * result + lineGraphEdgeCount
        return result
    }
}
