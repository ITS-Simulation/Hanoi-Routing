const STORAGE_KEY = "camera-config-web-state-v1";

const DEFAULT_STATE = {
  profiles: [
    {
      name: "arterial_rush",
      freeFlowKmh: 45.0,
      freeFlowOccupancy: 0.18,
      peaks: [
        { hour: 7.5, speedKmh: 14.0, occupancy: 0.78 },
        { hour: 17.5, speedKmh: 12.0, occupancy: 0.88 },
      ],
    },
    {
      name: "bridge_evening",
      freeFlowKmh: 55.0,
      freeFlowOccupancy: 0.12,
      peaks: [{ hour: 18.0, speedKmh: 16.0, occupancy: 0.92 }],
    },
    {
      name: "freeflow_local",
      freeFlowKmh: 35.0,
      freeFlowOccupancy: 0.2,
      peaks: [],
    },
  ],
  cameras: [],
};

const state = loadState();
let datasetStatus = null;
let map = null;
let clickMarker = null;
let selectedArcLayer = null;
let propagationLayer = null;
let cameraMarkers = [];
let nearbyCandidates = [];
let propagationPreview = null;
let propagationRequestToken = 0;
let activeTab = "search";
const draft = emptyDraft();

const elements = {
  datasetMeta: document.getElementById("dataset-meta"),
  tabButtons: [...document.querySelectorAll("[data-tab-target]")],
  tabPanels: [...document.querySelectorAll("[data-tab-panel]")],
  cameraWorkbench: document.getElementById("camera-workbench"),
  loadYamlBtn: document.getElementById("load-yaml-btn"),
  loadYamlInput: document.getElementById("load-yaml-input"),
  profilesList: document.getElementById("profiles-list"),
  addProfileBtn: document.getElementById("add-profile-btn"),
  roadSearchInput: document.getElementById("road-search-input"),
  roadSearchResults: document.getElementById("road-search-results"),
  cameraIdInput: document.getElementById("camera-id-input"),
  cameraLabelInput: document.getElementById("camera-label-input"),
  cameraProfileSelect: document.getElementById("camera-profile-select"),
  cameraArcIdInput: document.getElementById("camera-arc-id-input"),
  cameraBearingInput: document.getElementById("camera-bearing-input"),
  cameraLatInput: document.getElementById("camera-lat-input"),
  cameraLonInput: document.getElementById("camera-lon-input"),
  candidateList: document.getElementById("candidate-list"),
  selectedArcCard: document.getElementById("selected-arc-card"),
  propagationPreviewCard: document.getElementById("propagation-preview-card"),
  propagationArcList: document.getElementById("propagation-arc-list"),
  camerasList: document.getElementById("cameras-list"),
  exportYamlBtn: document.getElementById("export-yaml-btn"),
  downloadYamlBtn: document.getElementById("download-yaml-btn"),
  yamlOutput: document.getElementById("yaml-output"),
  deleteCameraBtn: document.getElementById("delete-camera-btn"),
  resetCameraBtn: document.getElementById("reset-camera-btn"),
  saveCameraBtn: document.getElementById("save-camera-btn"),
  editorBanner: document.getElementById("editor-banner"),
};

document.addEventListener("DOMContentLoaded", async () => {
  initMap();
  bindEvents();
  activateTab(activeTab);
  renderProfiles();
  renderProfileOptions();
  renderDraft();
  renderCameras();
  await loadDatasetStatus();
});

function emptyDraft() {
  return {
    editIndex: null,
    id: nextCameraId(),
    label: "",
    profile: state.profiles[0]?.name ?? "",
    placementMode: "arc",
    arcId: "",
    lat: "",
    lon: "",
    flowBearingDeg: "",
    selectedArc: null,
    displayLat: null,
    displayLon: null,
  };
}

function loadState() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) {
      return structuredClone(DEFAULT_STATE);
    }
    const parsed = JSON.parse(raw);
    return {
      profiles: Array.isArray(parsed.profiles) ? parsed.profiles : structuredClone(DEFAULT_STATE.profiles),
      cameras: Array.isArray(parsed.cameras) ? parsed.cameras : [],
    };
  } catch (_) {
    return structuredClone(DEFAULT_STATE);
  }
}

function saveState() {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
}

function initMap() {
  map = L.map("map");
  L.tileLayer("https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png", {
    maxZoom: 19,
    attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors',
  }).addTo(map);
  map.setView([21.0285, 105.8542], 13);

  map.on("click", async (event) => {
    activateTab("camera");
    draft.displayLat = event.latlng.lat;
    draft.displayLon = event.latlng.lng;
    if (draft.placementMode === "coordinate") {
      draft.lat = event.latlng.lat.toFixed(6);
      draft.lon = event.latlng.lng.toFixed(6);
    }
    renderDraft();
    setClickMarker(event.latlng.lat, event.latlng.lng);
    await loadNearbyCandidates(event.latlng.lat, event.latlng.lng);
  });
}

function bindEvents() {
  elements.tabButtons.forEach((button) => {
    button.addEventListener("click", () => activateTab(button.dataset.tabTarget));
  });

  elements.addProfileBtn.addEventListener("click", () => {
    state.profiles.push({
      name: `profile_${state.profiles.length + 1}`,
      freeFlowKmh: 40,
      freeFlowOccupancy: 0.2,
      peaks: [],
    });
    saveState();
    renderProfiles();
    renderProfileOptions();
    showBanner("Added a new profile card.", false);
  });

  elements.loadYamlBtn.addEventListener("click", () => {
    elements.loadYamlInput.click();
  });
  elements.loadYamlInput.addEventListener("change", handleYamlImportSelected);

  elements.profilesList.addEventListener("input", handleProfilesInput);
  elements.profilesList.addEventListener("click", handleProfilesClick);

  elements.roadSearchInput.addEventListener("input", debounce(handleRoadSearch, 180));
  elements.roadSearchResults.addEventListener("click", handleRoadSearchClick);

  elements.cameraIdInput.addEventListener("input", (event) => {
    draft.id = event.target.value;
  });
  elements.cameraLabelInput.addEventListener("input", (event) => {
    draft.label = event.target.value;
  });
  elements.cameraProfileSelect.addEventListener("change", (event) => {
    draft.profile = event.target.value;
  });
  elements.cameraBearingInput.addEventListener("input", (event) => {
    draft.flowBearingDeg = event.target.value;
    rerankNearbyCandidates();
  });
  elements.cameraLatInput.addEventListener("input", (event) => {
    draft.lat = event.target.value;
    draft.displayLat = numericOrNull(event.target.value);
  });
  elements.cameraLonInput.addEventListener("input", (event) => {
    draft.lon = event.target.value;
    draft.displayLon = numericOrNull(event.target.value);
  });

  document.querySelectorAll("input[name='placement-mode']").forEach((node) => {
    node.addEventListener("change", (event) => {
      draft.placementMode = event.target.value;
      if (draft.placementMode === "arc") {
        if (draft.selectedArc) {
          draft.arcId = String(draft.selectedArc.arc_id);
        }
      } else if (draft.selectedArc) {
        draft.lat = draft.displayLat?.toFixed(6) ?? draft.selectedArc.mid_lat.toFixed(6);
        draft.lon = draft.displayLon?.toFixed(6) ?? draft.selectedArc.mid_lon.toFixed(6);
        if (!draft.flowBearingDeg) {
          draft.flowBearingDeg = draft.selectedArc.bearing_deg.toFixed(1);
        }
      }
      renderDraft();
      rerankNearbyCandidates();
    });
  });

  elements.candidateList.addEventListener("click", handleCandidateClick);
  elements.camerasList.addEventListener("click", handleCameraListClick);
  elements.deleteCameraBtn.addEventListener("click", handleEditorDeleteCamera);
  elements.resetCameraBtn.addEventListener("click", resetDraft);
  elements.saveCameraBtn.addEventListener("click", saveCameraFromDraft);
  elements.exportYamlBtn.addEventListener("click", exportYaml);
  elements.downloadYamlBtn.addEventListener("click", downloadYaml);
}

function activateTab(tabName) {
  activeTab = tabName;
  elements.tabButtons.forEach((button) => {
    const isActive = button.dataset.tabTarget === tabName;
    button.classList.toggle("active", isActive);
    button.setAttribute("aria-pressed", isActive ? "true" : "false");
  });
  elements.tabPanels.forEach((panel) => {
    panel.classList.toggle("active", panel.dataset.tabPanel === tabName);
  });
  elements.cameraWorkbench.classList.toggle("hidden", tabName !== "camera");
}

async function handleYamlImportSelected(event) {
  const file = event.target.files?.[0];
  event.target.value = "";
  if (!file) {
    return;
  }

  if (hasMeaningfulEditorState()) {
    const shouldReplace = window.confirm(
      "Loading a YAML file will replace the current profiles and cameras in the editor. Continue?",
    );
    if (!shouldReplace) {
      return;
    }
  }

  try {
    const yamlText = await file.text();
    const response = await fetch("/api/import_yaml", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ yaml: yamlText }),
    });
    const payload = await response.json();
    if (!response.ok) {
      throw new Error(payload.error || "Could not load YAML.");
    }
    replaceEditorState(payload);
    elements.yamlOutput.value = yamlText;
    activateTab("saved");
    showBanner(
      `Loaded ${file.name}: ${payload.profiles.length} profile(s), ${payload.cameras.length} camera(s).`,
      false,
    );
  } catch (error) {
    showBanner(error.message, true);
  }
}

function hasMeaningfulEditorState() {
  if (state.cameras.length > 0) {
    return true;
  }
  if (state.profiles.length !== DEFAULT_STATE.profiles.length) {
    return true;
  }
  return JSON.stringify(state.profiles) !== JSON.stringify(DEFAULT_STATE.profiles);
}

function replaceEditorState(importedState) {
  state.profiles.splice(0, state.profiles.length, ...structuredClone(importedState.profiles ?? []));
  state.cameras.splice(0, state.cameras.length, ...structuredClone(importedState.cameras ?? []));
  saveState();
  renderProfiles();
  renderProfileOptions();
  renderCameras();
  resetDraft();
}

async function loadDatasetStatus() {
  const response = await fetch("/api/status");
  datasetStatus = await response.json();
  renderDatasetMeta();
  if (datasetStatus?.bounds) {
    map.fitBounds([
      [datasetStatus.bounds.minLat, datasetStatus.bounds.minLon],
      [datasetStatus.bounds.maxLat, datasetStatus.bounds.maxLon],
    ]);
  }
}

function renderDatasetMeta() {
  if (!datasetStatus) {
    return;
  }
  elements.datasetMeta.innerHTML = `
    <div><dt>Graph</dt><dd>${escapeHtml(datasetStatus.graphDir)}</dd></div>
    <div><dt>Arcs</dt><dd>${formatInt(datasetStatus.arcCount)}</dd></div>
  `;
}

function renderProfiles() {
  if (!state.profiles.length) {
    elements.profilesList.className = "stack empty";
    elements.profilesList.textContent = "No profiles yet.";
    return;
  }
  elements.profilesList.className = "stack";
  elements.profilesList.innerHTML = state.profiles
    .map((profile, index) => renderProfileCard(profile, index))
    .join("");
}

function renderProfileCard(profile, index) {
  return `
    <article class="profile-card" data-profile-index="${index}">
      <div class="section-title-row">
        <h3 data-role="profile-title">${escapeHtml(profile.name || `Profile ${index + 1}`)}</h3>
        <div class="button-row">
          <button type="button" data-action="duplicate-profile" data-profile-index="${index}">Duplicate</button>
          <button type="button" class="danger" data-action="remove-profile" data-profile-index="${index}">Remove</button>
        </div>
      </div>
      <label>
        <span>Name</span>
        <input type="text" data-field="name" data-profile-index="${index}" value="${escapeAttr(profile.name)}">
      </label>
      <div class="grid-two">
        <label>
          <span>Free-flow km/h</span>
          <input type="number" step="0.1" data-field="freeFlowKmh" data-profile-index="${index}" value="${profile.freeFlowKmh}">
        </label>
        <label>
          <span>Free-flow occupancy</span>
          <input type="number" min="0" max="1" step="0.01" data-field="freeFlowOccupancy" data-profile-index="${index}" value="${profile.freeFlowOccupancy}">
        </label>
      </div>
      <div class="section-title-row">
        <h3>Peaks</h3>
        <button type="button" data-action="add-peak" data-profile-index="${index}">Add Peak</button>
      </div>
      <div class="peak-table">
        ${profile.peaks.length ? profile.peaks.map((peak, peakIndex) => renderPeakRow(index, peak, peakIndex)).join("") : '<div class="muted">No peaks.</div>'}
      </div>
    </article>
  `;
}

function updateProfileCardTitle(profileIndex) {
  const card = elements.profilesList.querySelector(`.profile-card[data-profile-index="${profileIndex}"]`);
  const title = card?.querySelector("[data-role='profile-title']");
  if (!title) {
    return;
  }
  title.textContent = state.profiles[profileIndex]?.name || `Profile ${profileIndex + 1}`;
}

function duplicateProfileName(profileName) {
  const baseName = String(profileName || "profile").trim() || "profile";
  const copyBase = baseName.endsWith("_copy") ? baseName : `${baseName}_copy`;
  const existingNames = new Set(state.profiles.map((item) => String(item.name ?? "")));
  let candidate = copyBase;
  let suffix = 2;
  while (existingNames.has(candidate)) {
    candidate = `${copyBase}_${suffix}`;
    suffix += 1;
  }
  return candidate;
}

function renderPeakRow(profileIndex, peak, peakIndex) {
  return `
    <div class="peak-row">
      <div class="grid-two">
        <label>
          <span>Hour</span>
          <input type="number" step="0.1" data-action="peak-input" data-profile-index="${profileIndex}" data-peak-index="${peakIndex}" data-field="hour" value="${peak.hour}">
        </label>
        <label>
          <span>Speed km/h</span>
          <input type="number" step="0.1" data-action="peak-input" data-profile-index="${profileIndex}" data-peak-index="${peakIndex}" data-field="speedKmh" value="${peak.speedKmh}">
        </label>
      </div>
      <div class="grid-two">
        <label>
          <span>Occupancy</span>
          <input type="number" min="0" max="1" step="0.01" data-action="peak-input" data-profile-index="${profileIndex}" data-peak-index="${peakIndex}" data-field="occupancy" value="${peak.occupancy}">
        </label>
        <div class="button-row" style="align-items:end;">
          <button type="button" class="danger" data-action="remove-peak" data-profile-index="${profileIndex}" data-peak-index="${peakIndex}">Remove Peak</button>
        </div>
      </div>
    </div>
  `;
}

function renderProfileOptions() {
  const options = state.profiles
    .map((profile) => `<option value="${escapeAttr(profile.name)}">${escapeHtml(profile.name)}</option>`)
    .join("");
  elements.cameraProfileSelect.innerHTML = options;
  if (!state.profiles.some((profile) => profile.name === draft.profile)) {
    draft.profile = state.profiles[0]?.name ?? "";
  }
  elements.cameraProfileSelect.value = draft.profile;
}

function renderDraft() {
  elements.cameraIdInput.value = draft.id ?? "";
  elements.cameraLabelInput.value = draft.label ?? "";
  elements.cameraProfileSelect.value = draft.profile ?? "";
  elements.cameraArcIdInput.value = draft.arcId ?? "";
  elements.cameraBearingInput.value = draft.flowBearingDeg ?? "";
  elements.cameraLatInput.value = draft.lat ?? "";
  elements.cameraLonInput.value = draft.lon ?? "";
  document.querySelectorAll("input[name='placement-mode']").forEach((node) => {
    node.checked = node.value === draft.placementMode;
  });
  elements.cameraArcIdInput.disabled = draft.placementMode !== "arc";
  elements.cameraBearingInput.disabled = draft.placementMode !== "coordinate";
  elements.cameraLatInput.disabled = draft.placementMode !== "coordinate";
  elements.cameraLonInput.disabled = draft.placementMode !== "coordinate";
  elements.deleteCameraBtn.disabled = draft.editIndex == null;

  if (draft.selectedArc) {
    const directionText = candidateDirectionText(draft.selectedArc);
    const bearingMatch = candidateBearingDiff(draft.selectedArc);
    const representedCount =
      propagationPreview && propagationPreview.anchor_arc?.arc_id === draft.selectedArc.arc_id
        ? propagationPreview.covered_arc_count
        : null;
    elements.selectedArcCard.className = "arc-card active";
    elements.selectedArcCard.innerHTML = `
      <strong>${escapeHtml(draft.selectedArc.name || "(unnamed road)")}</strong>
      <div class="chip-row">
        <span class="chip">arc ${draft.selectedArc.arc_id}</span>
        <span class="chip">OSM ${draft.selectedArc.osm_way_id}</span>
        <span class="chip">${escapeHtml(draft.selectedArc.highway)}</span>
        <span class="chip">${draft.selectedArc.bearing_deg.toFixed(1)}°</span>
        <span class="chip">${escapeHtml(directionText)}</span>
        ${bearingMatch == null ? "" : `<span class="chip">flow match ${bearingMatch.toFixed(1)}°</span>`}
        ${representedCount == null ? "" : `<span class="chip">represents ${representedCount} arc(s)</span>`}
      </div>
    `;
  } else {
    elements.selectedArcCard.className = "arc-card empty";
    elements.selectedArcCard.textContent =
      "No directed arc selected yet. Click the map and choose the candidate whose bearing matches traffic flow.";
  }

}

function renderPropagationPreview() {
  if (!propagationPreview || !draft.selectedArc || propagationPreview.anchor_arc?.arc_id !== draft.selectedArc.arc_id) {
    elements.propagationPreviewCard.className = "arc-card empty";
    elements.propagationPreviewCard.textContent = "No propagation preview yet.";
    elements.propagationArcList.className = "result-list floating-list empty";
    elements.propagationArcList.textContent = "Select a directed arc to preview the represented way.";
    return;
  }

  const overlap = findPropagationConflict(
    propagationPreview.covered_arcs.map((arc) => arc.arc_id),
    draft.editIndex,
  );

  elements.propagationPreviewCard.className = "arc-card active";
  elements.propagationPreviewCard.innerHTML = `
    <strong>${escapeHtml(propagationPreview.name || "(unnamed road)")}</strong>
    <div class="chip-row">
      <span class="chip">way ${propagationPreview.routing_way_id}</span>
      <span class="chip">OSM ${propagationPreview.osm_way_id}</span>
      <span class="chip">${escapeHtml(propagationPreview.direction_label)}</span>
      <span class="chip">${propagationPreview.covered_arc_count} arc(s)</span>
      ${
        propagationPreview.warning_count
          ? `<span class="chip">${propagationPreview.warning_count} bearing warning(s)</span>`
          : ""
      }
    </div>
    ${
      overlap
        ? `<p class="muted">Overlap: arc ${overlap.overlappingArcId} is already represented by camera ${overlap.camera.id} (${escapeHtml(overlap.camera.label)}).</p>`
        : `<p class="muted">The selected anchor arc propagates to the way-direction group shown below.</p>`
    }
  `;

  elements.propagationArcList.className = "result-list floating-list";
  elements.propagationArcList.innerHTML = propagationPreview.covered_arcs
    .map((arc) => {
      const isAnchor = arc.arc_id === propagationPreview.anchor_arc.arc_id;
      const isWarning = Number(arc.bearing_diff_from_anchor_deg) > Number(propagationPreview.bearing_warn_threshold_deg);
      const classes = [
        "preview-arc-item",
        isAnchor ? "active" : "",
        isWarning ? "warning" : "",
      ]
        .filter(Boolean)
        .join(" ");
      return `
        <div class="${classes}">
          <strong>${escapeHtml(arc.name || "(unnamed road)")}</strong><br>
          arc ${arc.arc_id}${isAnchor ? " · anchor" : ""} · bearing ${Number(arc.bearing_deg).toFixed(1)}° · ${escapeHtml(candidateDirectionText(arc))}<br>
          diff from anchor ${Number(arc.bearing_diff_from_anchor_deg).toFixed(1)}°
        </div>
      `;
    })
    .join("");
}

function renderCandidates() {
  if (!nearbyCandidates.length) {
    elements.candidateList.className = "result-list floating-list empty";
    elements.candidateList.textContent = "Click the map to load candidates.";
    return;
  }

  elements.candidateList.className = "result-list floating-list";
  elements.candidateList.innerHTML = nearbyCandidates
    .map((candidate) => {
      const active = draft.selectedArc?.arc_id === candidate.arc_id ? "candidate-item active" : "candidate-item";
      const distanceText =
        typeof candidate.distance_m === "number" ? `${candidate.distance_m.toFixed(2)} m` : "manual selection";
      const directionText = candidateDirectionText(candidate);
      const bearingMatch = candidateBearingDiff(candidate);
      const assignedCamera = cameraUsingArc(candidate.arc_id, draft.editIndex);
      return `
        <button type="button" class="${active}" data-action="select-candidate" data-arc-id="${candidate.arc_id}">
          <strong>${escapeHtml(candidate.name || "(unnamed road)")}</strong><br>
          arc ${candidate.arc_id} · bearing ${candidate.bearing_deg.toFixed(1)}° · ${distanceText}<br>
          ${escapeHtml(directionText)}${bearingMatch == null ? "" : ` · flow match ${bearingMatch.toFixed(1)}°`}${
            assignedCamera ? `<br>already used by camera ${assignedCamera.id}: ${escapeHtml(assignedCamera.label)}` : ""
          }
        </button>
      `;
    })
    .join("");
}

function renderCameras() {
  renderCameraList();
  renderCameraMarkers();
}

function renderCameraList() {
  if (!state.cameras.length) {
    elements.camerasList.className = "stack empty";
    elements.camerasList.textContent = "No cameras yet.";
    return;
  }

  elements.camerasList.className = "stack";
  elements.camerasList.innerHTML = state.cameras
    .map((camera, index) => {
      const placement =
        camera.placementMode === "arc"
          ? `arc_id=${camera.arcId}`
          : `${Number(camera.lat).toFixed(6)}, ${Number(camera.lon).toFixed(6)} @ ${Number(camera.flowBearingDeg).toFixed(1)}°`;
      const selectedDirection = camera.selectedArc
        ? `selected arc ${camera.selectedArc.arc_id} · ${Number(camera.selectedArc.bearing_deg).toFixed(1)}° · ${candidateDirectionText(camera.selectedArc)}`
        : "";
      const representedWay = camera.representedWay
        ? `represents ${camera.representedWay.name || "(unnamed road)"} · ${camera.representedWay.directionLabel} · ${camera.representedWay.coveredArcCount} arc(s)`
        : "";
      return `
        <article class="camera-card">
          <div class="section-title-row">
            <h3>${escapeHtml(camera.label)}</h3>
            <div class="button-row">
              <button type="button" data-action="zoom-camera" data-index="${index}">Zoom</button>
              <button type="button" data-action="edit-camera" data-index="${index}">Edit</button>
              <button type="button" class="danger" data-action="delete-camera" data-index="${index}">Delete</button>
            </div>
          </div>
          <div class="chip-row">
            <span class="chip">id ${camera.id}</span>
            <span class="chip">${escapeHtml(camera.profile)}</span>
            <span class="chip">${escapeHtml(camera.placementMode)}</span>
          </div>
          <p class="muted">${escapeHtml(placement)}</p>
          ${selectedDirection ? `<p class="muted">${escapeHtml(selectedDirection)}</p>` : ""}
          ${representedWay ? `<p class="muted">${escapeHtml(representedWay)}</p>` : ""}
        </article>
      `;
    })
    .join("");
}

function focusCameraOnMap(camera) {
  const lat = Number(camera.displayLat);
  const lon = Number(camera.displayLon);
  if (!Number.isFinite(lat) || !Number.isFinite(lon)) {
    return false;
  }
  setClickMarker(lat, lon);
  map.flyTo([lat, lon], Math.max(map.getZoom(), 16));
  return true;
}

function editSavedCamera(index) {
  const camera = state.cameras[index];
  if (!camera) {
    return;
  }

  activateTab("camera");
  Object.assign(draft, emptyDraft(), structuredClone(camera), { editIndex: index });

  if (draft.selectedArc) {
    draft.selectedArc = structuredClone(draft.selectedArc);
    nearbyCandidates = [draft.selectedArc];
    highlightArc(draft.selectedArc);
    clearPropagationPreview();
    void loadPropagationPreview(draft.selectedArc.arc_id);
  } else {
    nearbyCandidates = [];
    clearSelectedArcLayer();
    clearPropagationPreview();
  }

  renderCandidates();
  focusCameraOnMap(camera);
  renderDraft();
  showBanner(`Editing camera '${camera.label}'.`, false);
}

function deleteSavedCamera(index) {
  const camera = state.cameras[index];
  if (!camera) {
    return;
  }

  state.cameras.splice(index, 1);
  saveState();
  renderCameras();
  resetDraft();
  showBanner(`Deleted camera '${camera.label}'.`, false);
}

function focusCameraPreview(camera) {
  focusCameraOnMap(camera);
  if (camera.selectedArc) {
    highlightArc(camera.selectedArc);
    void loadPropagationPreview(camera.selectedArc.arc_id);
  } else {
    clearSelectedArcLayer();
    clearPropagationPreview();
  }
}

function renderCameraMarkers() {
  cameraMarkers.forEach((marker) => marker.remove());
  cameraMarkers = state.cameras.flatMap((camera, index) => {
    const lat = Number(camera.displayLat);
    const lon = Number(camera.displayLon);
    if (!Number.isFinite(lat) || !Number.isFinite(lon)) {
      return [];
    }

    const marker = L.circleMarker([lat, lon], {
      radius: 7,
      weight: 2,
      color: "#0f7b6c",
      fillColor: "#0f7b6c",
      fillOpacity: 0.24,
    }).addTo(map);
    marker.bindPopup(`
      <strong>${escapeHtml(camera.label)}</strong><br>
      profile: ${escapeHtml(camera.profile)}<br>
      mode: ${escapeHtml(camera.placementMode)}
    `);
    marker.on("click", (event) => {
      event.originalEvent?.stopPropagation();
      editSavedCamera(index);
    });
    return [marker];
  });
}

async function loadNearbyCandidates(lat, lon) {
  const response = await fetch(`/api/nearby_arcs?lat=${encodeURIComponent(lat)}&lon=${encodeURIComponent(lon)}&limit=8`);
  const payload = await response.json();
  nearbyCandidates = [...(payload.candidates ?? [])].sort(compareCandidates);
  if (!nearbyCandidates.length) {
    draft.selectedArc = null;
    updateSelectedFieldsFromCandidate();
    clearSelectedArcLayer();
    clearPropagationPreview();
    renderDraft();
    renderCandidates();
    return;
  }

  if (shouldAutoSelectCandidate()) {
    selectCandidate(nearbyCandidates[0].arc_id, true);
    return;
  }

  draft.selectedArc = null;
  updateSelectedFieldsFromCandidate();
  clearSelectedArcLayer();
  clearPropagationPreview();
  renderDraft();
  renderCandidates();
}

function handleCandidateClick(event) {
  const button = event.target.closest("[data-action='select-candidate']");
  if (!button) {
    return;
  }
  const arcId = Number(button.dataset.arcId);
  selectCandidate(arcId);
}

function selectCandidate(arcId, silent = false) {
  const candidate = nearbyCandidates.find((item) => item.arc_id === arcId);
  if (!candidate) {
    return;
  }
  activateTab("camera");
  draft.selectedArc = candidate;
  updateSelectedFieldsFromCandidate();
  renderDraft();
  renderCandidates();
  highlightArc(candidate);
  clearPropagationPreview();
  void loadPropagationPreview(candidate.arc_id);
  if (!silent) {
    showBanner(`Selected arc ${candidate.arc_id} on ${candidate.name || "(unnamed road)"}.`, false);
  }
}

function rerankNearbyCandidates() {
  if (!nearbyCandidates.length) {
    return;
  }

  nearbyCandidates = [...nearbyCandidates].sort(compareCandidates);

  if (draft.selectedArc) {
    const refreshedSelection = nearbyCandidates.find((candidate) => candidate.arc_id === draft.selectedArc.arc_id);
    if (refreshedSelection) {
      draft.selectedArc = refreshedSelection;
    }
  }

  renderDraft();
  renderCandidates();
}

function updateSelectedFieldsFromCandidate() {
  if (!draft.selectedArc) {
    draft.arcId = "";
    if (draft.placementMode === "arc") {
      draft.flowBearingDeg = "";
    }
    return;
  }

  draft.arcId = String(draft.selectedArc.arc_id);
  if (draft.displayLat == null || draft.displayLon == null) {
    draft.displayLat = draft.selectedArc.mid_lat;
    draft.displayLon = draft.selectedArc.mid_lon;
  }
  if (draft.placementMode === "coordinate") {
    if (!draft.lat) {
      draft.lat = Number(draft.displayLat ?? draft.selectedArc.mid_lat).toFixed(6);
    }
    if (!draft.lon) {
      draft.lon = Number(draft.displayLon ?? draft.selectedArc.mid_lon).toFixed(6);
    }
    if (!draft.flowBearingDeg) {
      draft.flowBearingDeg = draft.selectedArc.bearing_deg.toFixed(1);
    }
  }
}

function compareCandidates(left, right) {
  const leftMatch = candidateBearingDiff(left);
  const rightMatch = candidateBearingDiff(right);

  if (leftMatch != null && rightMatch != null) {
    const scoreDelta = candidateScore(left, leftMatch) - candidateScore(right, rightMatch);
    if (Math.abs(scoreDelta) > 0.0001) {
      return scoreDelta;
    }
    if (Math.abs(leftMatch - rightMatch) > 0.0001) {
      return leftMatch - rightMatch;
    }
  }

  const distanceDelta = candidateDistance(left) - candidateDistance(right);
  if (Math.abs(distanceDelta) > 0.0001) {
    return distanceDelta;
  }

  return left.arc_id - right.arc_id;
}

function candidateScore(candidate, bearingMatch = candidateBearingDiff(candidate)) {
  return candidateDistance(candidate) + (bearingMatch == null ? 0 : bearingMatch * 2.0);
}

function candidateDistance(candidate) {
  return typeof candidate.distance_m === "number" ? candidate.distance_m : 0;
}

function candidateDirectionText(candidate) {
  return candidate.is_antiparallel_to_way ? "against OSM way" : "with OSM way";
}

function candidateBearingDiff(candidate) {
  if (!hasFlowBearing()) {
    return null;
  }
  return angleDiffDeg(Number(candidate.bearing_deg), Number(draft.flowBearingDeg));
}

function hasFlowBearing() {
  return draft.placementMode === "coordinate" && Number.isFinite(Number(draft.flowBearingDeg));
}

function shouldAutoSelectCandidate() {
  return hasFlowBearing() && nearbyCandidates.length > 0;
}

function angleDiffDeg(left, right) {
  const raw = Math.abs(normalizeBearingJs(left) - normalizeBearingJs(right));
  return Math.min(raw, 360 - raw);
}

function highlightArc(candidate) {
  clearSelectedArcLayer();
  const arrowIcon = createDirectionArrowIcon(
    candidate.tail_lat,
    candidate.tail_lon,
    candidate.head_lat,
    candidate.head_lon,
    "#d3532a",
  );
  selectedArcLayer = L.layerGroup([
    L.polyline(
      [
        [candidate.tail_lat, candidate.tail_lon],
        [candidate.head_lat, candidate.head_lon],
      ],
      {
        color: "#d3532a",
        weight: 8,
        opacity: 0.96,
      },
    ),
    L.marker([candidate.head_lat, candidate.head_lon], {
      icon: arrowIcon,
      keyboard: false,
      interactive: false,
    }),
  ]).addTo(map);
}

function highlightPropagationPreview(preview) {
  clearPropagationLayer();
  const layers = preview.covered_arcs.map((arc) =>
    L.polyline(
      [
        [arc.tail_lat, arc.tail_lon],
        [arc.head_lat, arc.head_lon],
      ],
      {
        color: "#2055d6",
        weight: arc.arc_id === preview.anchor_arc.arc_id ? 6 : 4.5,
        opacity: arc.arc_id === preview.anchor_arc.arc_id ? 0.92 : 0.62,
      },
    ),
  );
  const anchorArc = preview.covered_arcs.find((arc) => arc.arc_id === preview.anchor_arc.arc_id);
  if (anchorArc) {
    layers.push(
      L.marker([anchorArc.head_lat, anchorArc.head_lon], {
        icon: createDirectionArrowIcon(
          anchorArc.tail_lat,
          anchorArc.tail_lon,
          anchorArc.head_lat,
          anchorArc.head_lon,
          "#2055d6",
        ),
        keyboard: false,
        interactive: false,
      }),
    );
  }
  propagationLayer = L.layerGroup(layers).addTo(map);
}

function createDirectionArrowIcon(tailLat, tailLon, headLat, headLon, color) {
  const tailPoint = map.latLngToLayerPoint([tailLat, tailLon]);
  const headPoint = map.latLngToLayerPoint([headLat, headLon]);
  const dx = headPoint.x - tailPoint.x;
  const dy = headPoint.y - tailPoint.y;
  const rotationDeg = Math.hypot(dx, dy) < 0.001 ? 0 : Math.atan2(dy, dx) * (180 / Math.PI);
  return L.divIcon({
    className: "map-direction-arrow-icon",
    iconSize: [26, 26],
    iconAnchor: [24, 13],
    html: `
      <div
        class="map-direction-arrow"
        style="--arrow-color: ${escapeAttr(color)}; --arrow-rotation: ${rotationDeg}deg;"
      ></div>
    `,
  });
}

function clearSelectedArcLayer() {
  if (selectedArcLayer) {
    selectedArcLayer.remove();
    selectedArcLayer = null;
  }
}

function clearPropagationLayer() {
  if (propagationLayer) {
    propagationLayer.remove();
    propagationLayer = null;
  }
}

function clearPropagationPreview() {
  propagationRequestToken += 1;
  propagationPreview = null;
  clearPropagationLayer();
  renderPropagationPreview();
}

async function loadPropagationPreview(arcId) {
  const requestToken = ++propagationRequestToken;
  propagationPreview = null;
  clearPropagationLayer();
  renderPropagationPreview();
  try {
    const response = await fetch(`/api/propagation_preview?arc_id=${encodeURIComponent(arcId)}`);
    const payload = await response.json();
    if (requestToken !== propagationRequestToken) {
      return;
    }
    if (!response.ok) {
      throw new Error(payload.error || "Could not load propagation preview.");
    }
    propagationPreview = payload;
    renderDraft();
    renderPropagationPreview();
    highlightPropagationPreview(payload);
  } catch (error) {
    if (requestToken !== propagationRequestToken) {
      return;
    }
    propagationPreview = null;
    clearPropagationLayer();
    renderPropagationPreview();
    showBanner(error.message, true);
  }
}

function setClickMarker(lat, lon) {
  if (clickMarker) {
    clickMarker.remove();
  }
  clickMarker = L.marker([lat, lon]).addTo(map);
}

function handleProfilesInput(event) {
  const target = event.target;
  const profileIndex = Number(target.dataset.profileIndex);
  if (!Number.isInteger(profileIndex)) {
    return;
  }
  const profile = state.profiles[profileIndex];
  if (!profile) {
    return;
  }

  if (target.dataset.action === "peak-input") {
    const peakIndex = Number(target.dataset.peakIndex);
    const peak = profile.peaks[peakIndex];
    if (!peak) {
      return;
    }
    peak[target.dataset.field] = numericOrString(target.value);
    saveState();
    return;
  }

  if (target.dataset.field === "name") {
    const oldName = String(profile.name ?? "");
    const newName = String(target.value);
    profile.name = newName;
    state.cameras.forEach((camera) => {
      if (camera.profile === oldName) {
        camera.profile = newName;
      }
    });
    if (draft.profile === oldName) {
      draft.profile = newName;
    }
    saveState();
    updateProfileCardTitle(profileIndex);
    renderProfileOptions();
    renderCameras();
    return;
  }

  profile[target.dataset.field] = numericOrString(target.value);
  saveState();
  renderProfileOptions();
}

function handleProfilesClick(event) {
  const button = event.target.closest("button[data-action]");
  if (!button) {
    return;
  }
  const profileIndex = Number(button.dataset.profileIndex);
  if (!Number.isInteger(profileIndex)) {
    return;
  }
  const profile = state.profiles[profileIndex];
  if (!profile) {
    return;
  }

  if (button.dataset.action === "remove-profile") {
    const inUse = state.cameras.some((camera) => camera.profile === profile.name);
    if (inUse) {
      showBanner(`Profile '${profile.name}' is assigned to one or more cameras.`, true);
      return;
    }
    state.profiles.splice(profileIndex, 1);
    saveState();
    renderProfiles();
    renderProfileOptions();
    renderDraft();
    return;
  }

  if (button.dataset.action === "duplicate-profile") {
    const profileCopy = structuredClone(profile);
    profileCopy.name = duplicateProfileName(profile.name);
    state.profiles.splice(profileIndex + 1, 0, profileCopy);
    saveState();
    renderProfiles();
    renderProfileOptions();
    showBanner(`Duplicated profile '${profile.name}' as '${profileCopy.name}'.`, false);
    return;
  }

  if (button.dataset.action === "add-peak") {
    profile.peaks.push({ hour: 8.0, speedKmh: 15.0, occupancy: 0.7 });
    saveState();
    renderProfiles();
    return;
  }

  if (button.dataset.action === "remove-peak") {
    const peakIndex = Number(button.dataset.peakIndex);
    profile.peaks.splice(peakIndex, 1);
    saveState();
    renderProfiles();
  }
}

async function handleRoadSearch() {
  activateTab("search");
  const query = elements.roadSearchInput.value.trim();
  if (query.length < 2) {
    elements.roadSearchResults.className = "result-list scroll-list empty";
    elements.roadSearchResults.textContent = "Type at least 2 characters.";
    return;
  }

  const response = await fetch(`/api/search_roads?q=${encodeURIComponent(query)}&limit=20`);
  const payload = await response.json();
  const results = payload.results ?? [];
  if (!results.length) {
    elements.roadSearchResults.className = "result-list scroll-list empty";
    elements.roadSearchResults.textContent = "No matching roads.";
    return;
  }

  elements.roadSearchResults.className = "result-list scroll-list";
  elements.roadSearchResults.innerHTML = results
    .map(
      (result) => `
        <button type="button" class="search-item" data-action="select-search-result" data-arc-id="${result.sample_arc.arc_id}">
          <strong>${escapeHtml(result.name)}</strong><br>
          ${result.arc_count} arc(s) · sample arc ${result.sample_arc.arc_id}
        </button>
      `,
    )
    .join("");
}

function handleRoadSearchClick(event) {
  const button = event.target.closest("[data-action='select-search-result']");
  if (!button) {
    return;
  }
  const arcId = Number(button.dataset.arcId);
  fetch(`/api/arc?arc_id=${encodeURIComponent(arcId)}`)
    .then((response) => response.json())
    .then((arc) => {
      if (typeof arc.arc_id !== "number") {
        throw new Error(arc.error || "Could not load arc");
      }
      draft.displayLat = arc.mid_lat;
      draft.displayLon = arc.mid_lon;
      if (draft.placementMode === "coordinate") {
        draft.lat = arc.mid_lat.toFixed(6);
        draft.lon = arc.mid_lon.toFixed(6);
      }
      setClickMarker(arc.mid_lat, arc.mid_lon);
      map.flyTo([arc.mid_lat, arc.mid_lon], Math.max(map.getZoom(), 16));
      activateTab("camera");
      return loadNearbyCandidates(arc.mid_lat, arc.mid_lon);
    })
    .catch((error) => showBanner(error.message, true));
}

function handleEditorDeleteCamera() {
  if (draft.editIndex == null) {
    return;
  }
  deleteSavedCamera(draft.editIndex);
}

function handleCameraListClick(event) {
  const button = event.target.closest("button[data-action]");
  if (!button) {
    return;
  }
  const index = Number(button.dataset.index);
  const camera = state.cameras[index];
  if (!camera) {
    return;
  }

  if (button.dataset.action === "delete-camera") {
    deleteSavedCamera(index);
    return;
  }

  if (button.dataset.action === "edit-camera") {
    editSavedCamera(index);
    return;
  }

  if (button.dataset.action === "zoom-camera") {
    activateTab("camera");
    focusCameraPreview(camera);
  }
}

function saveCameraFromDraft() {
  try {
    const camera = normalizeDraftCamera();
    if (draft.editIndex == null) {
      state.cameras.push(camera);
      showBanner(`Added camera '${camera.label}'.`, false);
    } else {
      state.cameras.splice(draft.editIndex, 1, camera);
      showBanner(`Updated camera '${camera.label}'.`, false);
    }
    saveState();
    renderCameras();
    resetDraft();
  } catch (error) {
    showBanner(error.message, true);
  }
}

function normalizeDraftCamera() {
  const id = integerOrThrow(draft.id, "Camera ID");
  if (state.cameras.some((camera, index) => camera.id === id && index !== draft.editIndex)) {
    throw new Error(`Camera id ${id} is already in use.`);
  }
  const label = String(draft.label || "").trim();
  if (!label) {
    throw new Error("Camera label must not be blank.");
  }
  const profile = String(draft.profile || "").trim();
  if (!state.profiles.some((item) => item.name === profile)) {
    throw new Error("Choose an existing profile for the camera.");
  }
  if (!draft.selectedArc) {
    throw new Error("Select a nearby arc from the map before saving the camera.");
  }
  if (
    !propagationPreview ||
    Number(propagationPreview.anchor_arc?.arc_id) !== Number(draft.selectedArc.arc_id)
  ) {
    throw new Error("Propagation preview is still loading. Please wait a moment and try again.");
  }
  const representedArcIds = currentRepresentedArcIds();
  const propagationConflict = findPropagationConflict(representedArcIds, draft.editIndex);
  if (propagationConflict) {
    throw new Error(
      `Arc ${propagationConflict.overlappingArcId} is already represented by camera ${propagationConflict.camera.id} ('${propagationConflict.camera.label}').`,
    );
  }

  const camera = {
    id,
    label,
    profile,
    placementMode: draft.placementMode,
    selectedArc: structuredClone(draft.selectedArc),
    displayLat: Number.isFinite(Number(draft.displayLat)) ? Number(draft.displayLat) : draft.selectedArc.mid_lat,
    displayLon: Number.isFinite(Number(draft.displayLon)) ? Number(draft.displayLon) : draft.selectedArc.mid_lon,
    representedWay: propagationPreview && propagationPreview.anchor_arc?.arc_id === draft.selectedArc.arc_id
      ? {
          routingWayId: propagationPreview.routing_way_id,
          name: propagationPreview.name,
          directionLabel: propagationPreview.direction_label,
          coveredArcCount: propagationPreview.covered_arc_count,
          warningCount: propagationPreview.warning_count,
        }
      : null,
    propagatedArcIds: representedArcIds,
  };

  if (draft.placementMode === "arc") {
    camera.arcId = integerOrThrow(draft.arcId, "Arc ID");
  } else {
    camera.lat = boundedFloatOrThrow(draft.lat, "Latitude", -90, 90);
    camera.lon = boundedFloatOrThrow(draft.lon, "Longitude", -180, 180);
    camera.flowBearingDeg = normalizeBearingJs(floatOrThrow(draft.flowBearingDeg, "Flow bearing"));
  }

  return camera;
}

function resetDraft() {
  Object.assign(draft, emptyDraft());
  nearbyCandidates = [];
  clearSelectedArcLayer();
  clearPropagationPreview();
  if (clickMarker) {
    clickMarker.remove();
    clickMarker = null;
  }
  renderCandidates();
  renderDraft();
}

async function exportYaml() {
  try {
    activateTab("export");
    const payload = {
      profiles: state.profiles.map((profile) => ({
        name: String(profile.name || "").trim(),
        freeFlowKmh: floatOrThrow(profile.freeFlowKmh, `Profile '${profile.name}' free-flow km/h`),
        freeFlowOccupancy: floatOrThrow(profile.freeFlowOccupancy, `Profile '${profile.name}' free-flow occupancy`),
        peaks: profile.peaks.map((peak) => ({
          hour: floatOrThrow(peak.hour, `Profile '${profile.name}' peak hour`),
          speedKmh: floatOrThrow(peak.speedKmh, `Profile '${profile.name}' peak speed`),
          occupancy: floatOrThrow(peak.occupancy, `Profile '${profile.name}' peak occupancy`),
        })),
      })),
      cameras: state.cameras.map((camera) => ({
        id: camera.id,
        label: camera.label,
        profile: camera.profile,
        placementMode: camera.placementMode,
        arcId: camera.arcId,
        lat: camera.lat,
        lon: camera.lon,
        flowBearingDeg: camera.flowBearingDeg,
      })),
    };
    const response = await fetch("/api/export_yaml", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    const result = await response.json();
    if (!response.ok) {
      throw new Error(result.error || "Could not generate YAML.");
    }
    elements.yamlOutput.value = result.yaml;
    showBanner("Generated YAML successfully.", false);
  } catch (error) {
    showBanner(error.message, true);
  }
}

function downloadYaml() {
  if (!elements.yamlOutput.value.trim()) {
    showBanner("Generate YAML first.", true);
    return;
  }
  const blob = new Blob([elements.yamlOutput.value], { type: "text/yaml;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = "cameras.yaml";
  anchor.click();
  URL.revokeObjectURL(url);
}

function showBanner(message, isError = false, silent = false) {
  elements.editorBanner.textContent = message;
  elements.editorBanner.className = `banner${isError ? " error" : ""}`;
  if (!message) {
    elements.editorBanner.classList.add("hidden");
  } else {
    elements.editorBanner.classList.remove("hidden");
  }
}

function formatInt(value) {
  return new Intl.NumberFormat().format(value);
}

function escapeHtml(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function escapeAttr(value) {
  return escapeHtml(value).replaceAll("'", "&#39;");
}

function numericOrString(raw) {
  const trimmed = String(raw).trim();
  if (trimmed === "") {
    return "";
  }
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) ? parsed : trimmed;
}

function numericOrNull(raw) {
  const parsed = Number(raw);
  return Number.isFinite(parsed) ? parsed : null;
}

function integerOrThrow(raw, label) {
  const value = Number(raw);
  if (!Number.isInteger(value)) {
    throw new Error(`${label} must be an integer.`);
  }
  return value;
}

function floatOrThrow(raw, label) {
  const value = Number(raw);
  if (!Number.isFinite(value)) {
    throw new Error(`${label} must be a finite number.`);
  }
  return value;
}

function boundedFloatOrThrow(raw, label, min, max) {
  const value = floatOrThrow(raw, label);
  if (value < min || value > max) {
    throw new Error(`${label} must be in [${min}, ${max}].`);
  }
  return value;
}

function nextCameraId() {
  return state.cameras.reduce((max, camera) => Math.max(max, Number(camera.id) || 0), 0) + 1;
}

function cameraUsingArc(arcId, ignoreIndex = null) {
  return (
    state.cameras.find((camera, index) => {
      if (ignoreIndex != null && index === ignoreIndex) {
        return false;
      }
      return Number(camera.selectedArc?.arc_id) === Number(arcId);
    }) ?? null
  );
}

function cameraCoveredArcIds(camera) {
  if (Array.isArray(camera.propagatedArcIds) && camera.propagatedArcIds.length) {
    return camera.propagatedArcIds.map((arcId) => Number(arcId)).filter((arcId) => Number.isInteger(arcId));
  }
  const selectedArcId = Number(camera.selectedArc?.arc_id);
  return Number.isInteger(selectedArcId) ? [selectedArcId] : [];
}

function findPropagationConflict(representedArcIds, ignoreIndex = null) {
  const representedArcSet = new Set(representedArcIds.map((arcId) => Number(arcId)));
  for (let index = 0; index < state.cameras.length; index += 1) {
    if (ignoreIndex != null && index === ignoreIndex) {
      continue;
    }
    const camera = state.cameras[index];
    const overlap = cameraCoveredArcIds(camera).find((arcId) => representedArcSet.has(Number(arcId)));
    if (overlap != null) {
      return { camera, overlappingArcId: overlap };
    }
  }
  return null;
}

function currentRepresentedArcIds() {
  if (
    propagationPreview &&
    draft.selectedArc &&
    Number(propagationPreview.anchor_arc?.arc_id) === Number(draft.selectedArc.arc_id)
  ) {
    return propagationPreview.covered_arcs.map((arc) => Number(arc.arc_id));
  }
  if (draft.selectedArc) {
    return [Number(draft.selectedArc.arc_id)];
  }
  return [];
}

function normalizeBearingJs(value) {
  let normalized = value % 360;
  if (normalized < 0) {
    normalized += 360;
  }
  return normalized;
}

function debounce(fn, delay) {
  let timer = null;
  return (...args) => {
    clearTimeout(timer);
    timer = setTimeout(() => fn(...args), delay);
  };
}
