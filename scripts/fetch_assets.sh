#!/usr/bin/env bash
# Downloads the CC0 assets used by the aquarium (Poly Haven, https://polyhaven.com — all CC0).
# Run once; afterwards the project builds and runs fully offline.
#
#   ./scripts/fetch_assets.sh          # skip files already present
#   FORCE=1 ./scripts/fetch_assets.sh  # re-download everything
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/assets"
UA="aquarium-asset-fetch/0.1 (bevy aquarium demo)"

fetch() {
    local rel="$1" url="$2" out="$DEST/$1"
    if [[ -s "$out" && -z "${FORCE:-}" ]]; then
        return
    fi
    mkdir -p "$(dirname "$out")"
    echo "  ↓ $rel"
    curl -fsSL --retry 3 -A "$UA" "$url" -o "$out.part"
    mv "$out.part" "$out"
}

echo "Fetching CC0 assets into $DEST"

# --- Sand (Poly Haven texture "aerial_beach_01", 2k) ---
T=https://dl.polyhaven.org/file/ph-assets/Textures/jpg/2k/aerial_beach_01
fetch "textures/sand/sand_diff.jpg"   "$T/aerial_beach_01_diff_2k.jpg"
fetch "textures/sand/sand_nor_gl.jpg" "$T/aerial_beach_01_nor_gl_2k.jpg"
fetch "textures/sand/sand_arm.jpg"    "$T/aerial_beach_01_arm_2k.jpg"
fetch "textures/sand/sand_disp.jpg"   "$T/aerial_beach_01_disp_2k.jpg"

# --- Scanned rocks, driftwood and shell (Poly Haven models, glTF) ---
fetch "models/namaqualand_boulder_02/namaqualand_boulder_02.gltf" "https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/namaqualand_boulder_02/namaqualand_boulder_02_2k.gltf"
fetch "models/namaqualand_boulder_02/namaqualand_boulder_02.bin" "https://dl.polyhaven.org/file/ph-assets/Models/gltf/8k/namaqualand_boulder_02/namaqualand_boulder_02.bin"
fetch "models/namaqualand_boulder_02/textures/namaqualand_boulder_02_arm_2k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/2k/namaqualand_boulder_02/namaqualand_boulder_02_arm_2k.jpg"
fetch "models/namaqualand_boulder_02/textures/namaqualand_boulder_02_diff_2k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/2k/namaqualand_boulder_02/namaqualand_boulder_02_diff_2k.jpg"
fetch "models/namaqualand_boulder_02/textures/namaqualand_boulder_02_nor_gl_2k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/2k/namaqualand_boulder_02/namaqualand_boulder_02_nor_gl_2k.jpg"

fetch "models/namaqualand_boulder_03/namaqualand_boulder_03.gltf" "https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/namaqualand_boulder_03/namaqualand_boulder_03_2k.gltf"
fetch "models/namaqualand_boulder_03/namaqualand_boulder_03.bin" "https://dl.polyhaven.org/file/ph-assets/Models/gltf/8k/namaqualand_boulder_03/namaqualand_boulder_03.bin"
fetch "models/namaqualand_boulder_03/textures/namaqualand_boulder_03_arm_2k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/2k/namaqualand_boulder_03/namaqualand_boulder_03_arm_2k.jpg"
fetch "models/namaqualand_boulder_03/textures/namaqualand_boulder_03_diff_2k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/2k/namaqualand_boulder_03/namaqualand_boulder_03_diff_2k.jpg"
fetch "models/namaqualand_boulder_03/textures/namaqualand_boulder_03_nor_gl_2k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/2k/namaqualand_boulder_03/namaqualand_boulder_03_nor_gl_2k.jpg"

fetch "models/namaqualand_boulder_05/namaqualand_boulder_05.gltf" "https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/namaqualand_boulder_05/namaqualand_boulder_05_2k.gltf"
fetch "models/namaqualand_boulder_05/namaqualand_boulder_05.bin" "https://dl.polyhaven.org/file/ph-assets/Models/gltf/8k/namaqualand_boulder_05/namaqualand_boulder_05.bin"
fetch "models/namaqualand_boulder_05/textures/namaqualand_boulder_05_arm_2k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/2k/namaqualand_boulder_05/namaqualand_boulder_05_arm_2k.jpg"
fetch "models/namaqualand_boulder_05/textures/namaqualand_boulder_05_diff_2k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/2k/namaqualand_boulder_05/namaqualand_boulder_05_diff_2k.jpg"
fetch "models/namaqualand_boulder_05/textures/namaqualand_boulder_05_nor_gl_2k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/2k/namaqualand_boulder_05/namaqualand_boulder_05_nor_gl_2k.jpg"

fetch "models/rock_09/rock_09.gltf" "https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/rock_09/rock_09_2k.gltf"
fetch "models/rock_09/rock_09.bin" "https://dl.polyhaven.org/file/ph-assets/Models/gltf/8k/rock_09/rock_09.bin"
fetch "models/rock_09/textures/rock_09_arm_2k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/2k/rock_09/rock_09_arm_2k.jpg"
fetch "models/rock_09/textures/rock_09_diff_2k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/2k/rock_09/rock_09_diff_2k.jpg"
fetch "models/rock_09/textures/rock_09_nor_gl_2k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/2k/rock_09/rock_09_nor_gl_2k.jpg"

fetch "models/namaqualand_stones_01/namaqualand_stones_01.gltf" "https://dl.polyhaven.org/file/ph-assets/Models/gltf/1k/namaqualand_stones_01/namaqualand_stones_01_1k.gltf"
fetch "models/namaqualand_stones_01/namaqualand_stones_01.bin" "https://dl.polyhaven.org/file/ph-assets/Models/gltf/8k/namaqualand_stones_01/namaqualand_stones_01.bin"
fetch "models/namaqualand_stones_01/textures/namaqualand_stones_01_arm_1k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/1k/namaqualand_stones_01/namaqualand_stones_01_arm_1k.jpg"
fetch "models/namaqualand_stones_01/textures/namaqualand_stones_01_diff_1k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/1k/namaqualand_stones_01/namaqualand_stones_01_diff_1k.jpg"
fetch "models/namaqualand_stones_01/textures/namaqualand_stones_01_nor_gl_1k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/1k/namaqualand_stones_01/namaqualand_stones_01_nor_gl_1k.jpg"

fetch "models/namaqualand_rocks_01/namaqualand_rocks_01.gltf" "https://dl.polyhaven.org/file/ph-assets/Models/gltf/1k/namaqualand_rocks_01/namaqualand_rocks_01_1k.gltf"
fetch "models/namaqualand_rocks_01/namaqualand_rocks_01.bin" "https://dl.polyhaven.org/file/ph-assets/Models/gltf/8k/namaqualand_rocks_01/namaqualand_rocks_01.bin"
fetch "models/namaqualand_rocks_01/textures/namaqualand_rocks_01_arm_1k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/1k/namaqualand_rocks_01/namaqualand_rocks_01_arm_1k.jpg"
fetch "models/namaqualand_rocks_01/textures/namaqualand_rocks_01_diff_1k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/1k/namaqualand_rocks_01/namaqualand_rocks_01_diff_1k.jpg"
fetch "models/namaqualand_rocks_01/textures/namaqualand_rocks_01_nor_gl_1k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/1k/namaqualand_rocks_01/namaqualand_rocks_01_nor_gl_1k.jpg"

fetch "models/dead_quiver_branch_01/dead_quiver_branch_01.gltf" "https://dl.polyhaven.org/file/ph-assets/Models/gltf/2k/dead_quiver_branch_01/dead_quiver_branch_01_2k.gltf"
fetch "models/dead_quiver_branch_01/dead_quiver_branch_01.bin" "https://dl.polyhaven.org/file/ph-assets/Models/gltf/8k/dead_quiver_branch_01/dead_quiver_branch_01.bin"
fetch "models/dead_quiver_branch_01/textures/dead_quiver_branch_01_arm_2k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/2k/dead_quiver_branch_01/dead_quiver_branch_01_arm_2k.jpg"
fetch "models/dead_quiver_branch_01/textures/dead_quiver_branch_01_diff_2k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/2k/dead_quiver_branch_01/dead_quiver_branch_01_diff_2k.jpg"
fetch "models/dead_quiver_branch_01/textures/dead_quiver_branch_01_nor_gl_2k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/2k/dead_quiver_branch_01/dead_quiver_branch_01_nor_gl_2k.jpg"

fetch "models/lambis_shell/lambis_shell.gltf" "https://dl.polyhaven.org/file/ph-assets/Models/gltf/1k/lambis_shell/lambis_shell_1k.gltf"
fetch "models/lambis_shell/lambis_shell.bin" "https://dl.polyhaven.org/file/ph-assets/Models/gltf/8k/lambis_shell/lambis_shell.bin"
fetch "models/lambis_shell/textures/lambis_shell_arm_1k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/1k/lambis_shell/lambis_shell_arm_1k.jpg"
fetch "models/lambis_shell/textures/lambis_shell_diff_1k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/1k/lambis_shell/lambis_shell_diff_1k.jpg"
fetch "models/lambis_shell/textures/lambis_shell_nor_gl_1k.jpg" "https://dl.polyhaven.org/file/ph-assets/Models/jpg/1k/lambis_shell/lambis_shell_nor_gl_1k.jpg"

echo "Done. Assets in $DEST"
