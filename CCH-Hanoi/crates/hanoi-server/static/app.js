const STORAGE_KEY = "hanoi-server-route-viewer-state-v2";
const DEFAULT_CENTER = [21.0285, 105.8542];
const DEFAULT_ZOOM = 13;
const TRAFFIC_MIN_ZOOM = 14;
const TRAFFIC_REFRESH_INTERVAL_MS = 10000;
const MAX_IMPORTED_ROUTES = 10;
const COMPARE_ROUTE_COLORS = [
  "#d15f2f",
  "#0e8bd8",
  "#7b4dd8",
  "#129a73",
  "#d94668",
  "#c4861a",
  "#00897b",
  "#7f5af0",
  "#ea580c",
  "#2f6fed",
];

const TURN_LABELS = {
  straight: "Continue straight",
  slight_left: "Slight left",
  slight_right: "Slight right",
  left: "Turn left",
  right: "Turn right",
  sharp_left: "Sharp left",
  sharp_right: "Sharp right",
  u_turn: "Make a U-turn",
  roundabout_straight: "Roundabout, continue through",
  roundabout_slight_left: "Roundabout, slight left exit",
  roundabout_slight_right: "Roundabout, slight right exit",
  roundabout_left: "Roundabout, take the left exit",
  roundabout_right: "Roundabout, take the right exit",
  roundabout_sharp_left: "Roundabout, sharp left exit",
  roundabout_sharp_right: "Roundabout, sharp right exit",
  roundabout_u_turn: "Roundabout, loop back",
};

const state = loadState();
let map;
let routeHaloLayer;
let routeLineLayer;
let trafficRenderer;
let compareRenderer;
let trafficLayers = [];
let cameraLayers = [];
let compareRouteLayers = [];
let fromMarker = null;
let toMarker = null;
let serverInfo = null;
let healthInfo = null;
let readyInfo = null;
let lastRouteGeometryPointCount = null;
let trafficRefreshTimer = null;
let trafficOverlayRequestToken = 0;
let cameraOverlayRequestToken = 0;
let trafficTertiaryFilterSupported = true;
let cameraOverlayAvailable = true;
let queryRouteFeatureCollection = null;
let compareRoutes = [];
let compareRouteIdCounter = 0;

const elements = {
  legendCard: document.getElementById("legend-card"),
  legendCollapseBtn: document.getElementById("legend-collapse-btn"),
  queryForm: document.getElementById("query-form"),
  refreshServerBtn: document.getElementById("refresh-server-btn"),
  resetWeightsBtn: document.getElementById("reset-weights-btn"),
  workspaceQueryBtn: document.getElementById("workspace-query-btn"),
  workspaceCompareBtn: document.getElementById("workspace-compare-btn"),
  workspaceCaption: document.getElementById("workspace-caption"),
  queryPanel: document.getElementById("query-panel"),
  comparePanel: document.getElementById("compare-panel"),
  pickFromBtn: document.getElementById("pick-from-btn"),
  pickToBtn: document.getElementById("pick-to-btn"),
  fromCard: document.getElementById("from-card"),
  toCard: document.getElementById("to-card"),
  fromLatInput: document.getElementById("from-lat-input"),
  fromLngInput: document.getElementById("from-lng-input"),
  toLatInput: document.getElementById("to-lat-input"),
  toLngInput: document.getElementById("to-lng-input"),
  swapPointsBtn: document.getElementById("swap-points-btn"),
  resetPointsBtn: document.getElementById("reset-points-btn"),
  runQueryBtn: document.getElementById("run-query-btn"),
  exportRouteBtn: document.getElementById("export-route-btn"),
  messageBanner: document.getElementById("message-banner"),
  compareBanner: document.getElementById("compare-banner"),
  loadRouteFilesBtn: document.getElementById("load-route-files-btn"),
  loadRouteFilesInput: document.getElementById("load-route-files-input"),
  recalculateRoutesBtn: document.getElementById("recalculate-routes-btn"),
  clearRoutesBtn: document.getElementById("clear-routes-btn"),
  compareViewAllBtn: document.getElementById("compare-view-all-btn"),
  compareViewFocusBtn: document.getElementById("compare-view-focus-btn"),
  compareFocusControls: document.getElementById("compare-focus-controls"),
  compareFocusASelect: document.getElementById("compare-focus-a-select"),
  compareFocusBSelect: document.getElementById("compare-focus-b-select"),
  compareFocusSummary: document.getElementById("compare-focus-summary"),
  compareRouteCount: document.getElementById("compare-route-count"),
  compareRouteList: document.getElementById("compare-route-list"),
  pickerHint: document.getElementById("picker-hint"),
  mapOverlayTitle: document.getElementById("map-overlay-title"),
  mapOverlayCopy: document.getElementById("map-overlay-copy"),
  serverStatusChip: document.getElementById("server-status-chip"),
  graphTypeChip: document.getElementById("graph-type-chip"),
  metaNodes: document.getElementById("meta-nodes"),
  metaEdges: document.getElementById("meta-edges"),
  metaQueries: document.getElementById("meta-queries"),
  metaUptime: document.getElementById("meta-uptime"),
  coverageCaption: document.getElementById("coverage-caption"),
  routeBadge: document.getElementById("route-badge"),
  statTime: document.getElementById("stat-time"),
  statDistance: document.getElementById("stat-distance"),
  statTurns: document.getElementById("stat-turns"),
  statLatency: document.getElementById("stat-latency"),
  cameraToggleBtn: document.getElementById("camera-toggle-btn"),
  cameraOverlayStatus: document.getElementById("camera-overlay-status"),
  trafficToggleBtn: document.getElementById("traffic-toggle-btn"),
  trafficTertiaryFilterInput: document.getElementById("traffic-tertiary-filter-input"),
  trafficOverlayStatus: document.getElementById("traffic-overlay-status"),
  summaryFrom: document.getElementById("summary-from"),
  summaryTo: document.getElementById("summary-to"),
  summaryPoints: document.getElementById("summary-points"),
  summaryMode: document.getElementById("summary-mode"),
  turnList: document.getElementById("turn-list"),
};

document.addEventListener("DOMContentLoaded", async () => {
  initMap();
  bindEvents();
  renderInputs();
  renderLegendCardState();
  renderModeState();
  renderMarkers();
  renderCameraOverlayControls();
  renderTrafficOverlayControls();
  renderEmptyRouteState();
  renderCompareState();
  await refreshServerContext();
  syncTrafficOverlayPolling();
  if (state.cameraEnabled) {
    void refreshCameraOverlay();
  }
  if (state.trafficEnabled) {
    void refreshTrafficOverlay();
  }
});

function loadState() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) {
      return {
        activeTab: "query",
        compareView: "all",
        focusRouteAId: "",
        focusRouteBId: "",
        activeTarget: "from",
        fromLat: "",
        fromLng: "",
        toLat: "",
        toLng: "",
        legendCollapsed: false,
        cameraEnabled: false,
        trafficEnabled: false,
        trafficTertiaryAndAboveOnly: false,
      };
    }

    const parsed = JSON.parse(raw);
    return {
      activeTab: parsed.activeTab === "compare" ? "compare" : "query",
      compareView: parsed.compareView === "focus" ? "focus" : "all",
      focusRouteAId: typeof parsed.focusRouteAId === "string" ? parsed.focusRouteAId : "",
      focusRouteBId: typeof parsed.focusRouteBId === "string" ? parsed.focusRouteBId : "",
      activeTarget: parsed.activeTarget === "to" ? "to" : "from",
      fromLat: typeof parsed.fromLat === "string" ? parsed.fromLat : "",
      fromLng: typeof parsed.fromLng === "string" ? parsed.fromLng : "",
      toLat: typeof parsed.toLat === "string" ? parsed.toLat : "",
      toLng: typeof parsed.toLng === "string" ? parsed.toLng : "",
      legendCollapsed: parsed.legendCollapsed === true,
      cameraEnabled: parsed.cameraEnabled === true,
      trafficEnabled: parsed.trafficEnabled === true,
      trafficTertiaryAndAboveOnly: parsed.trafficTertiaryAndAboveOnly === true,
    };
  } catch (_) {
    return {
      activeTab: "query",
      compareView: "all",
        focusRouteAId: "",
        focusRouteBId: "",
        activeTarget: "from",
        fromLat: "",
      fromLng: "",
      toLat: "",
      toLng: "",
      legendCollapsed: false,
      cameraEnabled: false,
      trafficEnabled: false,
      trafficTertiaryAndAboveOnly: false,
    };
  }
}

function saveState() {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
}

function initMap() {
  map = L.map("map", { zoomControl: false });
  L.control.zoom({ position: "bottomright" }).addTo(map);

  L.tileLayer("https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png", {
    maxZoom: 19,
    attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors',
  }).addTo(map);

  map.setView(DEFAULT_CENTER, DEFAULT_ZOOM);

  map.createPane("traffic");
  map.getPane("traffic").style.zIndex = "420";
  map.createPane("cameraOverlay");
  map.getPane("cameraOverlay").style.zIndex = "426";
  map.createPane("compareRoute");
  map.getPane("compareRoute").style.zIndex = "425";
  map.createPane("routeHalo");
  map.getPane("routeHalo").style.zIndex = "430";
  map.createPane("routeLine");
  map.getPane("routeLine").style.zIndex = "431";
  trafficRenderer = L.canvas({ pane: "traffic", padding: 0.5 });
  compareRenderer = L.canvas({ pane: "compareRoute", padding: 0.5 });

  routeHaloLayer = L.geoJSON(null, {
    pane: "routeHalo",
    style: () => ({
      className: "route-halo",
      color: "#87ebc3",
      weight: 18,
      opacity: 0.28,
    }),
  }).addTo(map);

  routeLineLayer = L.geoJSON(null, {
    pane: "routeLine",
    style: () => ({
      className: "route-main",
      color: "#139a73",
      weight: 8,
      opacity: 0.94,
    }),
  }).addTo(map);

  map.on("click", handleMapClick);
  map.on("moveend", () => {
    void refreshCameraOverlay({ silent: true });
    void refreshTrafficOverlay({ silent: true });
  });
  map.on("zoomend", () => {
    void refreshCameraOverlay({ silent: true });
    void refreshTrafficOverlay({ silent: true });
  });
}

function bindEvents() {
  elements.legendCollapseBtn.addEventListener("click", handleLegendCollapse);
  elements.queryForm.addEventListener("submit", handleQuerySubmit);
  elements.refreshServerBtn.addEventListener("click", refreshServerContext);
  elements.resetWeightsBtn.addEventListener("click", handleResetWeights);
  elements.workspaceQueryBtn.addEventListener("click", () => setActiveTab("query"));
  elements.workspaceCompareBtn.addEventListener("click", () => setActiveTab("compare"));
  elements.swapPointsBtn.addEventListener("click", handleSwapPoints);
  elements.resetPointsBtn.addEventListener("click", handleResetPoints);
  elements.exportRouteBtn.addEventListener("click", handleExportRoute);
  elements.compareViewAllBtn.addEventListener("click", () => setCompareView("all"));
  elements.compareViewFocusBtn.addEventListener("click", () => setCompareView("focus"));
  elements.loadRouteFilesBtn.addEventListener("click", () => {
    elements.loadRouteFilesInput.click();
  });
  elements.loadRouteFilesInput.addEventListener("change", handleCompareFilesSelected);
  elements.compareFocusASelect.addEventListener("change", (event) => {
    state.focusRouteAId = event.target.value;
    ensureFocusSelections();
    renderCompareState();
    if (state.activeTab === "compare") {
      refreshDisplayedRoutes();
    }
    saveState();
  });
  elements.compareFocusBSelect.addEventListener("change", (event) => {
    state.focusRouteBId = event.target.value;
    ensureFocusSelections();
    renderCompareState();
    if (state.activeTab === "compare") {
      refreshDisplayedRoutes();
    }
    saveState();
  });
  elements.recalculateRoutesBtn.addEventListener("click", () => {
    void evaluateCompareRoutes();
  });
  elements.clearRoutesBtn.addEventListener("click", clearCompareRoutes);
  elements.cameraToggleBtn.addEventListener("click", handleCameraToggle);
  elements.trafficToggleBtn.addEventListener("click", handleTrafficToggle);
  elements.trafficTertiaryFilterInput.addEventListener("change", handleTrafficTertiaryFilterToggle);

  document.querySelectorAll("[data-target]").forEach((button) => {
    button.addEventListener("click", () => setActiveTarget(button.dataset.target));
  });

  document.querySelectorAll("[data-clear-point]").forEach((button) => {
    button.addEventListener("click", () => clearPoint(button.dataset.clearPoint));
  });

  elements.fromLatInput.addEventListener("input", (event) => {
    state.fromLat = event.target.value;
    invalidateRoutePreview();
    renderMarkers();
    saveState();
  });
  elements.fromLngInput.addEventListener("input", (event) => {
    state.fromLng = event.target.value;
    invalidateRoutePreview();
    renderMarkers();
    saveState();
  });
  elements.toLatInput.addEventListener("input", (event) => {
    state.toLat = event.target.value;
    invalidateRoutePreview();
    renderMarkers();
    saveState();
  });
  elements.toLngInput.addEventListener("input", (event) => {
    state.toLng = event.target.value;
    invalidateRoutePreview();
    renderMarkers();
    saveState();
  });
}

async function refreshServerContext() {
  setBanner("Refreshing server context…", "info");

  const [infoResult, readyResult, healthResult] = await Promise.allSettled([
    fetchJson("/info"),
    fetchReadyStatus(),
    fetchJson("/health"),
  ]);

  serverInfo = infoResult.status === "fulfilled" ? infoResult.value : null;
  readyInfo = readyResult.status === "fulfilled" ? readyResult.value : null;
  healthInfo = healthResult.status === "fulfilled" ? healthResult.value : null;

  renderServerContext();
  renderModeState();
  renderCompareState();

  if (serverInfo?.bbox) {
    const bounds = [
      [serverInfo.bbox.min_lat, serverInfo.bbox.min_lng],
      [serverInfo.bbox.max_lat, serverInfo.bbox.max_lng],
    ];
    map.fitBounds(bounds, { padding: [36, 36] });
  }

  if (readyInfo?.ready) {
    setBanner("Server ready. Select two points to run a coordinate query.", "success");
  } else if (readyResult.status === "rejected") {
    setBanner("Could not reach /ready. The API may still be starting or unavailable.", "warning");
  } else {
    setBanner("Server responded but is not ready to serve queries yet.", "warning");
  }
}

async function handleResetWeights() {
  elements.resetWeightsBtn.disabled = true;
  const previousLabel = elements.resetWeightsBtn.textContent;
  elements.resetWeightsBtn.textContent = "Resetting…";
  setBanner("Re-queuing baseline weights…", "info");
  if (state.activeTab === "compare") {
    setCompareBanner("Re-queuing baseline weights…", "info");
  }

  try {
    const response = await fetch("/reset_weights", { method: "POST" });
    const payload = await response.json();
    if (!response.ok || payload?.accepted !== true) {
      throw new Error(payload?.message || "Could not reset weights to baseline.");
    }

    if (compareRoutes.length > 0) {
      await evaluateCompareRoutes({
        bannerMessage: "Baseline weights were queued and imported routes were recalculated.",
      });
    } else if (state.activeTab === "compare") {
      setCompareBanner("Baseline weights were queued for re-application.", "success");
    }

    if (state.trafficEnabled) {
      await refreshTrafficOverlay();
    } else {
      renderTrafficOverlayControls();
    }
    setBanner("Baseline weights were queued for re-application.", "success");
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    setBanner(message, "error");
    if (state.activeTab === "compare") {
      setCompareBanner(message, "error");
    }
  } finally {
    elements.resetWeightsBtn.disabled = false;
    elements.resetWeightsBtn.textContent = previousLabel;
  }
}

function renderServerContext() {
  const ready = Boolean(readyInfo?.ready);
  const statusText = ready ? "Engine ready" : "Engine unavailable";
  elements.serverStatusChip.textContent = statusText;
  elements.serverStatusChip.className = `status-chip ${ready ? "online" : "offline"}`;
  elements.graphTypeChip.textContent = `Graph: ${formatGraphType(serverInfo?.graph_type)}`;

  elements.metaNodes.textContent = formatCount(serverInfo?.num_nodes);
  elements.metaEdges.textContent = formatCount(serverInfo?.num_edges);
  elements.metaQueries.textContent = formatCount(healthInfo?.total_queries_processed);
  elements.metaUptime.textContent = formatDurationSeconds(healthInfo?.uptime_seconds);

  if (serverInfo?.bbox) {
    elements.coverageCaption.textContent =
      `Coverage lat ${serverInfo.bbox.min_lat.toFixed(4)} to ${serverInfo.bbox.max_lat.toFixed(4)}, `
      + `lng ${serverInfo.bbox.min_lng.toFixed(4)} to ${serverInfo.bbox.max_lng.toFixed(4)}.`;
  } else {
    elements.coverageCaption.textContent = "Coverage bounds are not available for this dataset.";
  }
}

function renderInputs() {
  elements.fromLatInput.value = state.fromLat;
  elements.fromLngInput.value = state.fromLng;
  elements.toLatInput.value = state.toLat;
  elements.toLngInput.value = state.toLng;
}

function setActiveTab(tab) {
  state.activeTab = tab === "compare" ? "compare" : "query";
  renderModeState();
  saveState();
}

function renderModeState() {
  const isQueryTab = state.activeTab === "query";
  elements.workspaceQueryBtn.classList.toggle("active", isQueryTab);
  elements.workspaceCompareBtn.classList.toggle("active", !isQueryTab);
  elements.queryPanel.classList.toggle("active", isQueryTab);
  elements.comparePanel.classList.toggle("active", !isQueryTab);
  elements.workspaceCaption.textContent = isQueryTab
    ? "Pick two endpoints on the map, query the server, then export the route as GeoJSON with replay metadata."
    : "Load up to 10 exported GeoJSON routes and compare their travel time and distance under the current server weights.";

  renderPickerState();
  refreshDisplayedRoutes();
}

function setCompareView(view) {
  state.compareView = view === "focus" ? "focus" : "all";
  ensureFocusSelections();
  renderCompareState();
  if (state.activeTab === "compare") {
    refreshDisplayedRoutes();
  }
  saveState();
}

function setActiveTarget(target) {
  state.activeTarget = target === "to" ? "to" : "from";
  renderPickerState();
  saveState();
}

function renderPickerState() {
  const isQueryTab = state.activeTab === "query";
  const pickingSource = state.activeTarget === "from";
  elements.pickFromBtn.classList.toggle("active", pickingSource);
  elements.pickToBtn.classList.toggle("active", !pickingSource);
  elements.fromCard.classList.toggle("active", pickingSource);
  elements.toCard.classList.toggle("active", !pickingSource);

  if (!isQueryTab) {
    elements.pickerHint.textContent = "Map clicks are disabled while the Compare tab is active.";
    elements.mapOverlayTitle.textContent = "Compare imported routes";
    elements.mapOverlayCopy.textContent =
      "Load exported GeoJSON files to evaluate and visualize multiple routes under the same active traffic condition.";
    return;
  }

  const currentTargetLabel = pickingSource ? "source" : "destination";
  const nextTargetLabel = pickingSource ? "destination" : "source";
  elements.pickerHint.textContent = `Map clicks are currently targeting the ${currentTargetLabel} point.`;
  elements.mapOverlayTitle.textContent = `Click to set the ${currentTargetLabel}`;
  elements.mapOverlayCopy.textContent =
    `After placing the ${currentTargetLabel}, the picker advances to ${nextTargetLabel} so you can keep working from the map.`;
}

function renderLegendCardState() {
  elements.legendCard.classList.toggle("collapsed", state.legendCollapsed);
  elements.legendCollapseBtn.textContent = state.legendCollapsed ? "Expand" : "Collapse";
  elements.legendCollapseBtn.setAttribute("aria-expanded", String(!state.legendCollapsed));
}

function handleLegendCollapse() {
  state.legendCollapsed = !state.legendCollapsed;
  renderLegendCardState();
  saveState();
}

function renderTrafficOverlayControls(statusMessage = null) {
  elements.trafficToggleBtn.textContent = state.trafficEnabled ? "Hide" : "Show";
  elements.trafficToggleBtn.classList.toggle("primary-btn", state.trafficEnabled);
  elements.trafficToggleBtn.classList.toggle("secondary-btn", !state.trafficEnabled);
  elements.trafficTertiaryFilterInput.disabled = !trafficTertiaryFilterSupported;
  elements.trafficTertiaryFilterInput.checked =
    trafficTertiaryFilterSupported && state.trafficTertiaryAndAboveOnly;
  elements.trafficOverlayStatus.textContent = statusMessage ?? defaultTrafficOverlayStatus();
}

function renderCameraOverlayControls(statusMessage = null) {
  elements.cameraToggleBtn.textContent = state.cameraEnabled ? "Hide" : "Show";
  elements.cameraToggleBtn.classList.toggle("primary-btn", state.cameraEnabled);
  elements.cameraToggleBtn.classList.toggle("secondary-btn", !state.cameraEnabled);
  elements.cameraToggleBtn.disabled = !cameraOverlayAvailable;
  elements.cameraOverlayStatus.textContent = statusMessage ?? defaultCameraOverlayStatus();
}

function defaultCameraOverlayStatus() {
  if (!cameraOverlayAvailable) {
    return "Camera overlay is unavailable for the current server configuration.";
  }
  if (!state.cameraEnabled) {
    return "Camera overlay is off.";
  }
  return "Camera overlay is on and shows configured camera locations from the current YAML file.";
}

function defaultTrafficOverlayStatus() {
  if (!state.trafficEnabled) {
    return "Traffic overlay is off.";
  }
  if (map && map.getZoom() < TRAFFIC_MIN_ZOOM) {
    return `Zoom in to level ${TRAFFIC_MIN_ZOOM}+ to load road traffic coloring.`;
  }
  if (!trafficTertiaryFilterSupported) {
    return "Traffic overlay is on. Road-class filtering is unavailable for this dataset.";
  }
  if (serverInfo?.graph_type === "line_graph") {
    return state.trafficTertiaryAndAboveOnly
      ? "Traffic overlay is on for roads tertiary and above only, using pseudo-normal arc mapping for the line graph."
      : "Traffic overlay is on using pseudo-normal arc mapping for the line graph.";
  }
  return state.trafficTertiaryAndAboveOnly
    ? "Traffic overlay is on for roads tertiary and above only."
    : "Traffic overlay is on and compares live customized weights against baseline travel_time.";
}

function handleTrafficToggle() {
  state.trafficEnabled = !state.trafficEnabled;
  saveState();
  syncTrafficOverlayPolling();
  if (!state.trafficEnabled) {
    trafficOverlayRequestToken += 1;
    clearTrafficLayers();
    renderTrafficOverlayControls();
    return;
  }
  renderTrafficOverlayControls("Loading traffic overlay…");
  void refreshTrafficOverlay();
}

function handleCameraToggle() {
  state.cameraEnabled = !state.cameraEnabled;
  saveState();
  if (!state.cameraEnabled) {
    cameraOverlayRequestToken += 1;
    clearCameraLayers();
    renderCameraOverlayControls();
    return;
  }
  renderCameraOverlayControls("Loading camera overlay…");
  void refreshCameraOverlay();
}

function handleTrafficTertiaryFilterToggle(event) {
  state.trafficTertiaryAndAboveOnly = event.target.checked;
  saveState();

  if (!state.trafficEnabled) {
    renderTrafficOverlayControls();
    return;
  }

  renderTrafficOverlayControls("Loading traffic overlay…");
  void refreshTrafficOverlay();
}

function syncTrafficOverlayPolling() {
  if (trafficRefreshTimer) {
    window.clearInterval(trafficRefreshTimer);
    trafficRefreshTimer = null;
  }
  if (state.trafficEnabled) {
    trafficRefreshTimer = window.setInterval(() => {
      void refreshTrafficOverlay({ silent: true });
    }, TRAFFIC_REFRESH_INTERVAL_MS);
  }
}

function clearTrafficLayers() {
  trafficLayers.forEach((layer) => layer.remove());
  trafficLayers = [];
}

function clearCameraLayers() {
  cameraLayers.forEach((layer) => layer.remove());
  cameraLayers = [];
}

async function refreshCameraOverlay({ silent = false } = {}) {
  if (!state.cameraEnabled) {
    clearCameraLayers();
    renderCameraOverlayControls();
    return;
  }

  const bounds = map.getBounds();
  const query = new URLSearchParams({
    min_lat: String(bounds.getSouth()),
    max_lat: String(bounds.getNorth()),
    min_lng: String(bounds.getWest()),
    max_lng: String(bounds.getEast()),
  });
  const requestToken = ++cameraOverlayRequestToken;

  if (!silent) {
    renderCameraOverlayControls("Loading camera overlay…");
  }

  try {
    const response = await fetch(`/camera_overlay?${query.toString()}`);
    const payload = await response.json();
    if (requestToken !== cameraOverlayRequestToken) {
      return;
    }
    if (!response.ok) {
      throw new Error(payload.error || payload.message || "Could not load the camera overlay.");
    }
    applyCameraOverlay(payload);
  } catch (error) {
    if (requestToken !== cameraOverlayRequestToken) {
      return;
    }
    clearCameraLayers();
    renderCameraOverlayControls(error instanceof Error ? error.message : String(error));
  }
}

async function refreshTrafficOverlay({ silent = false } = {}) {
  if (!state.trafficEnabled) {
    clearTrafficLayers();
    renderTrafficOverlayControls();
    return;
  }

  if (map.getZoom() < TRAFFIC_MIN_ZOOM) {
    trafficOverlayRequestToken += 1;
    clearTrafficLayers();
    renderTrafficOverlayControls();
    return;
  }

  const bounds = map.getBounds();
  const query = new URLSearchParams({
    min_lat: String(bounds.getSouth()),
    max_lat: String(bounds.getNorth()),
    min_lng: String(bounds.getWest()),
    max_lng: String(bounds.getEast()),
    tertiary_and_above_only: String(state.trafficTertiaryAndAboveOnly),
  });
  const requestToken = ++trafficOverlayRequestToken;

  if (!silent) {
    renderTrafficOverlayControls("Loading traffic overlay…");
  }

  try {
    const response = await fetch(`/traffic_overlay?${query.toString()}`);
    const payload = await response.json();
    if (requestToken !== trafficOverlayRequestToken) {
      return;
    }
    if (!response.ok) {
      throw new Error(payload.error || payload.message || "Could not load the traffic overlay.");
    }
    applyTrafficOverlay(payload);
  } catch (error) {
    if (requestToken !== trafficOverlayRequestToken) {
      return;
    }
    clearTrafficLayers();
    renderTrafficOverlayControls(error instanceof Error ? error.message : String(error));
  }
}

function applyCameraOverlay(payload) {
  clearCameraLayers();
  cameraOverlayAvailable = payload?.available !== false;

  if (!cameraOverlayAvailable) {
    state.cameraEnabled = false;
    saveState();
    renderCameraOverlayControls(
      payload?.message || "Camera overlay is unavailable for the current server configuration.",
    );
    return;
  }

  const cameras = Array.isArray(payload?.cameras) ? payload.cameras : [];

  for (const camera of cameras) {
    const lat = Number(camera?.lat);
    const lng = Number(camera?.lng);
    if (!Number.isFinite(lat) || !Number.isFinite(lng)) {
      continue;
    }

    const label = typeof camera?.label === "string" && camera.label.trim()
      ? camera.label.trim()
      : "Camera";
    const idRow = camera?.id == null ? "" : `<div><strong>ID</strong>: ${escapeHtml(String(camera.id))}</div>`;
    const profileRow = typeof camera?.profile === "string" && camera.profile.trim()
      ? `<div><strong>Profile</strong>: ${escapeHtml(camera.profile.trim())}</div>`
      : "";
    const arcRow = camera?.arc_id == null ? "" : `<div><strong>Arc</strong>: ${escapeHtml(String(camera.arc_id))}</div>`;

    const marker = L.circleMarker([lat, lng], {
      pane: "cameraOverlay",
      className: "camera-overlay",
      radius: 5,
      color: "#fff4dc",
      weight: 1.5,
      fillColor: "#d4622f",
      fillOpacity: 0.94,
    }).addTo(map);

    marker.bindPopup(
      `<div><strong>${escapeHtml(label)}</strong>${idRow}${profileRow}${arcRow}<div><strong>Location</strong>: ${formatCoordinate(lat, lng)}</div></div>`,
    );
    cameraLayers.push(marker);
  }

  const totalCameraCount = Number(payload?.total_camera_count) || 0;
  const visibleCameraCount = cameraLayers.length;

  if (totalCameraCount === 0) {
    renderCameraOverlayControls("Camera overlay is on, but the configured YAML file does not contain any cameras.");
    return;
  }

  if (visibleCameraCount === 0) {
    renderCameraOverlayControls(
      `Camera overlay on · no configured cameras in the current view (${formatCount(totalCameraCount)} total loaded).`,
    );
    return;
  }

  renderCameraOverlayControls(
    `Camera overlay on · ${formatCount(visibleCameraCount)} camera(s) visible · ${formatCount(totalCameraCount)} total loaded.`,
  );
}

function applyTrafficOverlay(payload) {
  clearTrafficLayers();
  trafficTertiaryFilterSupported = payload?.tertiary_filter_supported !== false;

  const buckets = Array.isArray(payload?.buckets) ? payload.buckets : [];
  for (const bucket of buckets) {
    if (!Array.isArray(bucket.segments) || bucket.segments.length === 0) {
      continue;
    }
    const layer = L.polyline(bucket.segments, {
      pane: "traffic",
      renderer: trafficRenderer,
      className: "traffic-overlay",
      color: bucket.color || "#34c26b",
      weight: 3,
      opacity: 0.82,
      lineCap: "round",
      lineJoin: "round",
    }).addTo(map);
    trafficLayers.push(layer);
  }

  const visibleSegmentCount = Number(payload?.visible_segment_count) || 0;
  const mappingMode = payload?.mapping_mode === "line_graph_pseudo_normal" ? "pseudo-normal line graph" : "normal graph";
  const trafficMode = payload?.using_customized_weights ? "Customized traffic" : "Baseline traffic";
  const filterLabel = payload?.tertiary_and_above_only ? " · tertiary+ only" : "";

  if (!trafficTertiaryFilterSupported && state.trafficTertiaryAndAboveOnly) {
    renderTrafficOverlayControls("Road-class filtering is unavailable for this dataset.");
    return;
  }

  if (visibleSegmentCount === 0) {
    renderTrafficOverlayControls(`No visible traffic segments in the current view (${mappingMode}${filterLabel}).`);
    return;
  }

  renderTrafficOverlayControls(
    `${trafficMode} overlay on${filterLabel} · ${formatCount(visibleSegmentCount)} segment(s) visible · ${mappingMode}.`,
  );
}

function renderMarkers() {
  const from = getPoint("from");
  const to = getPoint("to");

  if (from) {
    if (!fromMarker) {
      fromMarker = L.marker([from.lat, from.lng], {
        icon: markerIcon("S", "route-pin-from"),
      }).addTo(map);
    } else {
      fromMarker.setLatLng([from.lat, from.lng]);
    }
    fromMarker.bindPopup(`<strong>Source</strong><br>${formatCoordinate(from.lat, from.lng)}`);
  } else if (fromMarker) {
    map.removeLayer(fromMarker);
    fromMarker = null;
  }

  if (to) {
    if (!toMarker) {
      toMarker = L.marker([to.lat, to.lng], {
        icon: markerIcon("D", "route-pin-to"),
      }).addTo(map);
    } else {
      toMarker.setLatLng([to.lat, to.lng]);
    }
    toMarker.bindPopup(`<strong>Destination</strong><br>${formatCoordinate(to.lat, to.lng)}`);
  } else if (toMarker) {
    map.removeLayer(toMarker);
    toMarker = null;
  }
}

function markerIcon(label, className) {
  return L.divIcon({
    className: "route-pin-shell",
    html: `<div class="route-pin ${className}"><span>${label}</span></div>`,
    iconSize: [26, 38],
    iconAnchor: [13, 34],
    popupAnchor: [0, -28],
  });
}

function handleMapClick(event) {
  if (state.activeTab !== "query") {
    setCompareBanner("Map clicks only place source and destination points on the Query tab.", "info");
    return;
  }

  const { lat, lng } = event.latlng;
  assignPoint(state.activeTarget, lat, lng);
  setBanner(
    `${capitalize(state.activeTarget)} set to ${formatCoordinate(lat, lng)}.`,
    "info",
  );
  setActiveTarget(state.activeTarget === "from" ? "to" : "from");
}

function assignPoint(target, lat, lng) {
  const latValue = lat.toFixed(6);
  const lngValue = lng.toFixed(6);

  if (target === "to") {
    state.toLat = latValue;
    state.toLng = lngValue;
  } else {
    state.fromLat = latValue;
    state.fromLng = lngValue;
  }

  invalidateRoutePreview();
  renderInputs();
  renderMarkers();
  saveState();
}

function clearPoint(target) {
  if (target === "to") {
    state.toLat = "";
    state.toLng = "";
  } else {
    state.fromLat = "";
    state.fromLng = "";
  }

  invalidateRoutePreview();
  renderInputs();
  renderMarkers();
  saveState();
}

function handleSwapPoints() {
  [state.fromLat, state.toLat] = [state.toLat, state.fromLat];
  [state.fromLng, state.toLng] = [state.toLng, state.fromLng];
  invalidateRoutePreview();
  renderInputs();
  renderMarkers();
  saveState();
  setBanner("Source and destination were swapped.", "info");
}

function handleResetPoints() {
  state.fromLat = "";
  state.fromLng = "";
  state.toLat = "";
  state.toLng = "";
  queryRouteFeatureCollection = null;
  lastRouteGeometryPointCount = null;
  renderInputs();
  renderMarkers();
  refreshDisplayedRoutes();
  renderEmptyRouteState();
  saveState();
  setBanner("Route points and map overlays were cleared.", "info");
}

async function handleQuerySubmit(event) {
  event.preventDefault();

  const payload = buildQueryPayload();
  if (!payload) {
    return;
  }

  elements.runQueryBtn.disabled = true;
  elements.runQueryBtn.textContent = "Routing…";
  setBanner("Querying hanoi_server for GeoJSON…", "info");

  const startedAt = performance.now();

  try {
    const response = await fetch("/query", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });

    const data = await response.json();
    const latencyMs = performance.now() - startedAt;

    if (!response.ok) {
      throw buildErrorMessage(data);
    }

    applyRouteResult(data, latencyMs);
  } catch (error) {
    queryRouteFeatureCollection = null;
    refreshDisplayedRoutes();
    renderEmptyRouteState();
    setBanner(error instanceof Error ? error.message : String(error), "error");
  } finally {
    elements.runQueryBtn.disabled = false;
    elements.runQueryBtn.textContent = "Find Route";
  }
}

function buildQueryPayload() {
  const from = getPoint("from");
  const to = getPoint("to");

  if (!from || !to) {
    setBanner("Both source and destination coordinates are required.", "warning");
    return null;
  }

  if (!isFiniteCoordinate(from.lat, from.lng) || !isFiniteCoordinate(to.lat, to.lng)) {
    setBanner("Coordinates must be finite numeric values.", "warning");
    return null;
  }

  return {
    from_lat: from.lat,
    from_lng: from.lng,
    to_lat: to.lat,
    to_lng: to.lng,
  };
}

function applyRouteResult(featureCollection, latencyMs) {
  const normalized = normalizeRouteGeojson(featureCollection);
  const feature = normalized.features[0] ?? null;
  const geometry = feature?.geometry ?? null;
  const properties = feature?.properties ?? {};
  const turns = Array.isArray(properties.turns) ? properties.turns : [];
  const coordinates = Array.isArray(geometry?.coordinates) ? geometry.coordinates : [];
  lastRouteGeometryPointCount = coordinates.length;
  queryRouteFeatureCollection = normalized;
  refreshDisplayedRoutes();

  if (!geometry || geometry.type !== "LineString" || coordinates.length === 0) {
    renderEmptyRouteState();
    elements.statLatency.textContent = `${Math.round(latencyMs)} ms`;
    elements.summaryFrom.textContent = formatCoordinateValue(state.fromLat, state.fromLng);
    elements.summaryTo.textContent = formatCoordinateValue(state.toLat, state.toLng);
    setBanner("The server returned GeoJSON, but no route geometry was found for this pair.", "warning");
    return;
  }

  if (state.activeTab === "query") {
    const bounds = routeLineLayer.getBounds();
    if (bounds.isValid()) {
      map.fitBounds(bounds, { padding: [48, 48] });
    }
  }

  elements.routeBadge.textContent = "Route ready";
  elements.routeBadge.className = "soft-chip";
  elements.statTime.textContent = formatTravelTime(properties.distance_ms);
  elements.statDistance.textContent = formatDistance(properties.distance_m);
  elements.statTurns.textContent = String(turns.length);
  elements.statLatency.textContent = `${Math.round(latencyMs)} ms`;
  elements.summaryFrom.textContent = formatCoordinateValue(state.fromLat, state.fromLng);
  elements.summaryTo.textContent = formatCoordinateValue(state.toLat, state.toLng);
  elements.summaryPoints.textContent = formatCount(coordinates.length);
  elements.summaryMode.textContent = `GeoJSON / ${formatGraphType(properties.graph_type).toLowerCase()}`;

  renderTurns(turns);
  updateExportRouteButton();
  setBanner(
    `Route loaded: ${formatTravelTime(properties.distance_ms)} over ${formatDistance(properties.distance_m)}.`,
    "success",
  );
}

function renderEmptyRouteState() {
  elements.routeBadge.textContent = "Awaiting query";
  elements.routeBadge.className = "soft-chip";
  elements.statTime.textContent = "—";
  elements.statDistance.textContent = "—";
  elements.statTurns.textContent = "—";
  elements.statLatency.textContent = "—";
  elements.summaryFrom.textContent = formatCoordinateValue(state.fromLat, state.fromLng);
  elements.summaryTo.textContent = formatCoordinateValue(state.toLat, state.toLng);
  elements.summaryPoints.textContent = lastRouteGeometryPointCount == null ? "—" : formatCount(lastRouteGeometryPointCount);
  elements.summaryMode.textContent = "GeoJSON / coordinates";
  elements.turnList.className = "turn-list empty";
  elements.turnList.textContent = "No route queried yet.";
  updateExportRouteButton();
}

function invalidateRoutePreview() {
  queryRouteFeatureCollection = null;
  lastRouteGeometryPointCount = null;
  refreshDisplayedRoutes();
  renderEmptyRouteState();
}

function renderTurns(turns) {
  if (!turns.length) {
    elements.turnList.className = "turn-list empty";
    elements.turnList.textContent =
      serverInfo?.graph_type === "line_graph"
        ? "No maneuver annotations were returned for this route."
        : "This server is running in normal graph mode, so maneuver annotations are not included.";
    return;
  }

  elements.turnList.className = "turn-list";
  elements.turnList.innerHTML = turns
    .map((turn, index) => {
      const title = TURN_LABELS[turn.direction] ?? humanizeSnakeCase(turn.direction);
      return `
        <article class="turn-item">
          <div class="turn-item-head">
            <p class="turn-title">${index + 1}. ${title}</p>
            <span class="turn-angle">${formatSignedAngle(turn.angle_degrees)}</span>
          </div>
          <p class="turn-distance">Continue for ${formatDistance(turn.distance_to_next_m)} before the next maneuver.</p>
        </article>
      `;
    })
    .join("");
}

function updateExportRouteButton() {
  elements.exportRouteBtn.disabled = !(queryRouteFeatureCollection?.features?.length > 0);
}

function clearRouteLayers() {
  routeHaloLayer.clearLayers();
  routeLineLayer.clearLayers();
}

function clearCompareRouteLayers() {
  compareRouteLayers.forEach((layer) => layer.remove());
  compareRouteLayers = [];
}

function refreshDisplayedRoutes() {
  clearRouteLayers();
  clearCompareRouteLayers();

  if (state.activeTab === "query" && queryRouteFeatureCollection) {
    routeHaloLayer.addData(queryRouteFeatureCollection);
    routeLineLayer.addData(queryRouteFeatureCollection);
  }

  if (state.activeTab === "compare" && compareRoutes.length > 0) {
    renderCompareRouteLayers(getVisibleCompareRoutes());
  }
}

function renderCompareRouteLayers(routesToRender) {
  clearCompareRouteLayers();

  const bounds = [];
  for (const route of routesToRender) {
    const layer = L.geoJSON(route.geojson, {
      pane: "compareRoute",
      renderer: compareRenderer,
      style: () => ({
        className: "compare-route",
        color: route.color,
        weight: 6,
        opacity: 0.92,
        lineCap: "round",
        lineJoin: "round",
      }),
    }).addTo(map);
    compareRouteLayers.push(layer);

    const layerBounds = layer.getBounds();
    if (layerBounds.isValid()) {
      bounds.push(layerBounds);
    }
  }

  if (bounds.length > 0) {
    const combined = bounds[0];
    for (let index = 1; index < bounds.length; index += 1) {
      combined.extend(bounds[index]);
    }
    map.fitBounds(combined, { padding: [48, 48] });
  }
}

function normalizeRouteGeojson(value) {
  if (!value || typeof value !== "object") {
    throw new Error("GeoJSON must be a JSON object.");
  }

  if (value.type === "FeatureCollection") {
    const feature = Array.isArray(value.features)
      ? value.features.find((item) => item?.geometry?.type === "LineString")
      : null;
    if (!feature) {
      throw new Error("GeoJSON FeatureCollection must contain a LineString feature.");
    }
    return {
      type: "FeatureCollection",
      features: [feature],
    };
  }

  if (value.type === "Feature") {
    if (value.geometry?.type !== "LineString") {
      throw new Error("GeoJSON feature must use a LineString geometry.");
    }
    return {
      type: "FeatureCollection",
      features: [value],
    };
  }

  if (value.type === "LineString") {
    return {
      type: "FeatureCollection",
      features: [
        {
          type: "Feature",
          geometry: value,
          properties: {},
        },
      ],
    };
  }

  throw new Error("Unsupported GeoJSON type. Expected FeatureCollection, Feature, or LineString.");
}

function handleExportRoute() {
  if (!queryRouteFeatureCollection?.features?.length) {
    setBanner("Run a query first so there is a route to export.", "warning");
    return;
  }

  const timestamp = new Date().toISOString().replaceAll(":", "-");
  downloadJson(queryRouteFeatureCollection, `route-${timestamp}.geojson`);
  setBanner("Exported the current route as GeoJSON.", "success");
}

async function handleCompareFilesSelected(event) {
  const files = Array.from(event.target.files ?? []);
  elements.loadRouteFilesInput.value = "";

  if (!files.length) {
    return;
  }

  if (compareRoutes.length + files.length > MAX_IMPORTED_ROUTES) {
    setCompareBanner(`You can compare at most ${MAX_IMPORTED_ROUTES} GeoJSON routes at once.`, "warning");
    return;
  }

  elements.loadRouteFilesBtn.disabled = true;
  setCompareBanner("Reading selected GeoJSON files…", "info");

  try {
    const loadedRoutes = await Promise.all(
      files.map(async (file) => {
        const text = await file.text();
        let parsed;
        try {
          parsed = JSON.parse(text);
        } catch (_) {
          throw new Error(`${file.name} is not valid JSON.`);
        }

        return {
          id: `compare-route-${compareRouteIdCounter += 1}`,
          name: file.name,
          geojson: normalizeRouteGeojson(parsed),
        };
      }),
    );

    compareRoutes = compareRoutes.concat(loadedRoutes);
    assignCompareRouteColors();
    await evaluateCompareRoutes({
      bannerMessage: `Loaded ${loadedRoutes.length} GeoJSON route${loadedRoutes.length === 1 ? "" : "s"} and recalculated them.`,
    });
  } catch (error) {
    setCompareBanner(error instanceof Error ? error.message : String(error), "error");
  } finally {
    elements.loadRouteFilesBtn.disabled = false;
  }
}

function assignCompareRouteColors() {
  compareRoutes = compareRoutes.map((route, index) => ({
    ...route,
    color: COMPARE_ROUTE_COLORS[index % COMPARE_ROUTE_COLORS.length],
  }));
}

async function evaluateCompareRoutes({ bannerMessage = null } = {}) {
  if (!compareRoutes.length) {
    renderCompareState();
    return;
  }

  elements.recalculateRoutesBtn.disabled = true;
  elements.clearRoutesBtn.disabled = true;
  setCompareBanner("Evaluating imported GeoJSON routes against the active server weights…", "info");

  try {
    const response = await fetch("/evaluate_routes", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        routes: compareRoutes.map((route) => ({
          name: route.name,
          geojson: route.geojson,
        })),
      }),
    });
    const payload = await response.json();
    if (!response.ok) {
      throw buildErrorMessage(payload);
    }

    compareRoutes = compareRoutes.map((route, index) => ({
      ...route,
      result: payload.routes[index] ?? null,
    }));
    renderCompareState(payload);
    if (state.activeTab === "compare") {
      refreshDisplayedRoutes();
    }
    setCompareBanner(
      bannerMessage || buildCompareBannerMessage(payload),
      compareRoutes.some((route) => route.result?.error) ? "warning" : "success",
    );
  } catch (error) {
    setCompareBanner(error instanceof Error ? error.message : String(error), "error");
    renderCompareState();
  } finally {
    elements.recalculateRoutesBtn.disabled = false;
    elements.clearRoutesBtn.disabled = compareRoutes.length === 0;
  }
}

function clearCompareRoutes() {
  compareRoutes = [];
  state.focusRouteAId = "";
  state.focusRouteBId = "";
  clearCompareRouteLayers();
  renderCompareState();
  if (state.activeTab === "compare") {
    setCompareBanner("Imported GeoJSON routes were cleared.", "info");
  }
  saveState();
}

function renderCompareState(payload = null) {
  ensureFocusSelections();
  elements.compareViewAllBtn.classList.toggle("active", state.compareView !== "focus");
  elements.compareViewFocusBtn.classList.toggle("active", state.compareView === "focus");
  elements.compareFocusControls.classList.toggle("active", state.compareView === "focus");
  elements.compareFocusSummary.classList.toggle("active", state.compareView === "focus");

  renderCompareFocusControls();
  renderCompareFocusSummary();

  const routesToRender = getVisibleCompareRoutes();
  const routeCountLabel = state.compareView === "focus" && compareRoutes.length > 0
    ? `${routesToRender.length} focused / ${compareRoutes.length} loaded`
    : `${compareRoutes.length} route${compareRoutes.length === 1 ? "" : "s"}`;
  elements.compareRouteCount.textContent = routeCountLabel;
  elements.recalculateRoutesBtn.disabled = compareRoutes.length === 0;
  elements.clearRoutesBtn.disabled = compareRoutes.length === 0;

  if (!compareRoutes.length) {
    elements.compareRouteList.className = "compare-route-list empty";
    elements.compareRouteList.textContent = "No imported GeoJSON routes yet.";
    if (payload == null) {
      setCompareBanner("No GeoJSON routes loaded yet.", "info");
    }
    return;
  }

  elements.compareRouteList.className = "compare-route-list";

  elements.compareRouteList.innerHTML = routesToRender
    .map((route) => {
      const result = route.result ?? {};
      const error = typeof result.error === "string" ? result.error : null;
      const distanceText = formatDistance(result.distance_m);
      const travelTimeText = formatTravelTime(result.travel_time_ms);
      const exportLabel = result.export_graph_type
        ? `${formatGraphType(result.export_graph_type)} export`
        : "GeoJSON route";

      return `
        <article class="compare-route-card">
          <div class="compare-route-head">
            <div class="compare-route-name">
              <span class="compare-route-swatch" style="background:${route.color}"></span>
              <strong>${escapeHtml(route.name)}</strong>
            </div>
            <span class="soft-chip">${escapeHtml(exportLabel)}</span>
          </div>

          <div class="compare-metric-grid">
            <div class="compare-metric">
              <span class="detail-label">Travel Time</span>
              <span class="detail-value">${travelTimeText}</span>
            </div>
            <div class="compare-metric">
              <span class="detail-label">Distance</span>
              <span class="detail-value">${distanceText}</span>
            </div>
            <div class="compare-metric">
              <span class="detail-label">Geometry Points</span>
              <span class="detail-value">${formatCount(result.geometry_point_count)}</span>
            </div>
            <div class="compare-metric">
              <span class="detail-label">Route Arcs</span>
              <span class="detail-value">${formatCount(result.route_arc_count)}</span>
            </div>
            <div class="compare-metric">
              <span class="detail-label">Evaluation</span>
              <span class="detail-value">${formatTravelTimeMode(result.travel_time_mode)}</span>
            </div>
            <div class="compare-metric">
              <span class="detail-label">Distance Source</span>
              <span class="detail-value">${formatDistanceMode(result.distance_mode)}</span>
            </div>
          </div>

          ${error ? `<p class="compare-route-note compare-route-note-error">${escapeHtml(error)}</p>` : ""}
        </article>
      `;
    })
    .join("");
}

function setCompareBanner(message, tone) {
  elements.compareBanner.textContent = message;
  elements.compareBanner.className = `banner ${tone}`;
}

function buildCompareBannerMessage(payload) {
  const customized = payload?.using_customized_weights ? "customized" : "baseline";
  const graphType = formatGraphType(payload?.graph_type).toLowerCase();
  const modeLabel = state.compareView === "focus" ? "focus 1-1 mode" : "all-routes mode";
  return `Compared ${compareRoutes.length} route${compareRoutes.length === 1 ? "" : "s"} using the ${customized} weight profile on the ${graphType} in ${modeLabel}.`;
}

function ensureFocusSelections() {
  if (compareRoutes.length === 0) {
    state.focusRouteAId = "";
    state.focusRouteBId = "";
    return;
  }

  if (!compareRoutes.some((route) => route.id === state.focusRouteAId)) {
    state.focusRouteAId = compareRoutes[0]?.id ?? "";
  }

  if (!compareRoutes.some((route) => route.id === state.focusRouteBId) || state.focusRouteBId === state.focusRouteAId) {
    const fallbackRoute = compareRoutes.find((route) => route.id !== state.focusRouteAId);
    state.focusRouteBId = fallbackRoute?.id ?? "";
  }
}

function renderCompareFocusControls() {
  const optionsMarkup = compareRoutes
    .map((route) => `<option value="${escapeHtml(route.id)}">${escapeHtml(route.name)}</option>`)
    .join("");

  elements.compareFocusASelect.innerHTML = optionsMarkup;
  elements.compareFocusBSelect.innerHTML = optionsMarkup;
  elements.compareFocusASelect.value = state.focusRouteAId;
  elements.compareFocusBSelect.value = state.focusRouteBId;
}

function renderCompareFocusSummary() {
  if (state.compareView !== "focus") {
    elements.compareFocusSummary.className = "compare-focus-summary";
    elements.compareFocusSummary.textContent = "";
    return;
  }

  if (compareRoutes.length < 2) {
    elements.compareFocusSummary.className = "compare-focus-summary active empty";
    elements.compareFocusSummary.textContent = "Load at least two routes to use focus 1-1 comparison.";
    return;
  }

  const routeA = compareRoutes.find((route) => route.id === state.focusRouteAId) ?? null;
  const routeB = compareRoutes.find((route) => route.id === state.focusRouteBId) ?? null;
  if (!routeA || !routeB) {
    elements.compareFocusSummary.className = "compare-focus-summary active empty";
    elements.compareFocusSummary.textContent = "Select two routes to focus the comparison.";
    return;
  }

  elements.compareFocusSummary.className = "compare-focus-summary active";
  elements.compareFocusSummary.innerHTML = `
    <p class="compare-focus-head">${escapeHtml(routeA.name)} vs ${escapeHtml(routeB.name)}</p>
    <div class="compare-focus-grid">
      <div class="compare-focus-item">
        <span class="detail-label">Faster Route</span>
        <span class="detail-value">${describePairWinner(routeA, routeB, "travel_time_ms", formatTravelTime)}</span>
      </div>
      <div class="compare-focus-item">
        <span class="detail-label">Shorter Route</span>
        <span class="detail-value">${describePairWinner(routeA, routeB, "distance_m", formatDistance)}</span>
      </div>
      <div class="compare-focus-item">
        <span class="detail-label">Travel Time Gap</span>
        <span class="detail-value">${formatPairGap(routeA, routeB, "travel_time_ms", formatTravelTime)}</span>
      </div>
      <div class="compare-focus-item">
        <span class="detail-label">Distance Gap</span>
        <span class="detail-value">${formatPairGap(routeA, routeB, "distance_m", formatDistance)}</span>
      </div>
    </div>
  `;
}

function getVisibleCompareRoutes() {
  if (state.compareView !== "focus") {
    return compareRoutes;
  }

  const focusedIds = new Set([state.focusRouteAId, state.focusRouteBId].filter(Boolean));
  return compareRoutes.filter((route) => focusedIds.has(route.id));
}

function describePairWinner(routeA, routeB, field, formatter) {
  const valueA = routeA?.result?.[field];
  const valueB = routeB?.result?.[field];
  if (!Number.isFinite(valueA) || !Number.isFinite(valueB)) {
    return "Unavailable";
  }
  if (Math.abs(valueA - valueB) < 1e-9) {
    return "Same";
  }
  const winner = valueA < valueB ? routeA : routeB;
  return `${winner.name} by ${formatter(Math.abs(valueA - valueB))}`;
}

function formatPairGap(routeA, routeB, field, formatter) {
  const valueA = routeA?.result?.[field];
  const valueB = routeB?.result?.[field];
  if (!Number.isFinite(valueA) || !Number.isFinite(valueB)) {
    return "—";
  }
  return formatter(Math.abs(valueA - valueB));
}

function getPoint(target) {
  const lat = target === "to" ? state.toLat : state.fromLat;
  const lng = target === "to" ? state.toLng : state.fromLng;
  const parsedLat = Number.parseFloat(lat);
  const parsedLng = Number.parseFloat(lng);

  if (!Number.isFinite(parsedLat) || !Number.isFinite(parsedLng)) {
    return null;
  }

  return { lat: parsedLat, lng: parsedLng };
}

function isFiniteCoordinate(lat, lng) {
  return Number.isFinite(lat) && Number.isFinite(lng);
}

function setBanner(message, tone) {
  elements.messageBanner.textContent = message;
  elements.messageBanner.className = `banner ${tone}`;
}

async function fetchJson(url) {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`Request failed for ${url}: ${response.status}`);
  }
  return response.json();
}

async function fetchReadyStatus() {
  const response = await fetch("/ready");
  const payload = await response.json().catch(() => null);

  if (response.ok) {
    return payload;
  }

  if (response.status === 503 && payload && typeof payload.ready === "boolean") {
    return payload;
  }

  throw new Error(`Request failed for /ready: ${response.status}`);
}

function buildErrorMessage(payload) {
  const message = payload?.message || payload?.error || "Request failed.";
  const details = payload?.details;

  if (!details || typeof details !== "object") {
    return new Error(message);
  }

  if (details.reason === "out_of_bounds" && details.bbox) {
    return new Error(
      `${message} Coverage lat ${details.bbox.min_lat.toFixed(4)} to ${details.bbox.max_lat.toFixed(4)}, `
      + `lng ${details.bbox.min_lng.toFixed(4)} to ${details.bbox.max_lng.toFixed(4)}.`,
    );
  }

  if (details.reason === "snap_too_far" && Number.isFinite(details.snap_distance_m)) {
    return new Error(
      `${message} Nearest road is about ${formatDistance(details.snap_distance_m)} away.`,
    );
  }

  return new Error(message);
}

function formatTravelTime(distanceMs) {
  if (!Number.isFinite(distanceMs)) {
    return "—";
  }

  const totalSeconds = Math.round(distanceMs / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;

  if (hours > 0) {
    return `${hours}h ${minutes}m`;
  }
  if (minutes > 0) {
    return `${minutes}m ${seconds}s`;
  }
  return `${seconds}s`;
}

function formatDistance(distanceM) {
  if (!Number.isFinite(distanceM)) {
    return "—";
  }

  if (distanceM >= 1000) {
    return `${(distanceM / 1000).toFixed(2)} km`;
  }
  return `${distanceM.toFixed(2)} m`;
}

function formatTravelTimeMode(value) {
  switch (value) {
    case "exact_weight_path":
      return "Exact replay";
    case "normal_arc_sum":
      return "Normal arc sum";
    case "line_graph_pseudo_normal":
      return "Pseudo-normal";
    default:
      return "Unavailable";
  }
}

function formatDistanceMode(value) {
  switch (value) {
    case "route_arc_ids":
      return "Route arcs";
    case "path_nodes":
      return "Path nodes";
    case "weight_path_ids":
      return "Replay path";
    case "geometry":
      return "Geometry";
    default:
      return "Unavailable";
  }
}

function formatCoordinate(lat, lng) {
  return `${lat.toFixed(6)}, ${lng.toFixed(6)}`;
}

function formatCoordinateValue(lat, lng) {
  const parsedLat = Number.parseFloat(lat);
  const parsedLng = Number.parseFloat(lng);
  if (!Number.isFinite(parsedLat) || !Number.isFinite(parsedLng)) {
    return "—";
  }
  return formatCoordinate(parsedLat, parsedLng);
}

function formatGraphType(value) {
  if (!value) {
    return "—";
  }
  return value === "line_graph" ? "Line graph" : "Normal graph";
}

function formatCount(value) {
  return Number.isFinite(value) ? new Intl.NumberFormat().format(value) : "—";
}

function formatDurationSeconds(value) {
  if (!Number.isFinite(value)) {
    return "—";
  }

  const seconds = Math.round(value);
  if (seconds < 60) {
    return `${seconds}s`;
  }
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) {
    return `${minutes}m`;
  }
  const hours = Math.floor(minutes / 60);
  const remainingMinutes = minutes % 60;
  return `${hours}h ${remainingMinutes}m`;
}

function formatSignedAngle(value) {
  if (!Number.isFinite(value)) {
    return "Angle unavailable";
  }
  const rounded = Math.round(value);
  return `${rounded > 0 ? "+" : ""}${rounded}°`;
}

function humanizeSnakeCase(value) {
  if (typeof value !== "string" || !value) {
    return "Maneuver";
  }
  return value
    .split("_")
    .map((part) => capitalize(part))
    .join(" ");
}

function capitalize(value) {
  return value ? value.charAt(0).toUpperCase() + value.slice(1) : "";
}

function downloadJson(payload, fileName) {
  const blob = new Blob([JSON.stringify(payload, null, 2)], { type: "application/geo+json;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = fileName;
  document.body.append(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}

function escapeHtml(value) {
  if (typeof value !== "string") {
    return "";
  }
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}
