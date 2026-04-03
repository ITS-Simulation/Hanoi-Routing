# Camera Config Web

Small local web app for building `cameras.yaml` files for `CCH_Data_Pipeline`.

It uses:
- `road_arc_manifest.arrow` as the road/arc source of truth
- OpenStreetMap tiles as the map background
- the same camera/profile schema as
  [CCH_Data_Pipeline/examples/cameras.sample.yaml](/home/thomas/VTS/Hanoi-Routing/CCH_Data_Pipeline/examples/cameras.sample.yaml)

## Features

- click on the map to inspect nearby directed road arcs
- search roads by name from the manifest, including accent-insensitive queries
- compare nearby candidates by directed arc bearing and `with/against OSM way`
  labeling so two-way roads can be disambiguated safely
- preview the propagated way-direction group a selected child arc will
  represent, including all sibling arcs on that way/direction
- create and edit speed profiles
- create cameras in either:
  - explicit `arc_id` mode
  - coordinate mode with `lat`, `lon`, and `flow_bearing_deg`
- assign profiles to cameras
- prevent saving cameras whose propagated coverage overlaps an existing saved
  camera in the local editor state
- export a YAML file compatible with the current pipeline loader

## Run

```bash
python3 camera-config-web/server.py --graph-dir Maps/data/hanoi_motorcycle/graph
```

Then open:

```text
http://127.0.0.1:8765
```

`--graph-dir` may point either to:
- a graph directory containing `road_arc_manifest.arrow`
- or a dataset root containing `graph/road_arc_manifest.arrow`

## Notes

- The app keeps the editable UI state in browser local storage.
- Leaflet and the tile layer are loaded from public CDNs / OpenStreetMap tile
  servers, so the browser needs internet access for the map background.
- The backend expects `pyarrow`, `numpy`, and `PyYAML`.
