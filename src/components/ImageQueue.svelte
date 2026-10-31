<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { ArrowDown, ArrowUp, X } from "lucide-svelte";
  let { files, onfiles }: { files: string[]; onfiles: (files: string[]) => void } = $props();
  function move(index: number, delta: number) {
    const target = index + delta;
    if (target < 0 || target >= files.length) return;
    // Renvoie une nouvelle liste pour que le parent Svelte observe le changement.
    const next = [...files]; [next[index], next[target]] = [next[target], next[index]]; onfiles(next);
  }
  function name(path: string) { return path.split(/[\\/]/).pop() || path; }
</script>

<div class="file-list thumbnail-list">
  {#each files as file, i (file)}
    <div class="file-row thumbnail-row">
      <div class="file-index">{String(i + 1).padStart(2, "0")}</div>
      <!-- Convertit le chemin local en URL que la vue WebView est autorisée à lire. -->
      <img class="queue-thumb" src={convertFileSrc(file)} alt="" />
      <div class="file-name" title={file}>{name(file)}</div>
      <div class="reorder">
        <button aria-label="Monter" disabled={i === 0} onclick={() => move(i, -1)}><ArrowUp size={14} /></button>
        <button aria-label="Descendre" disabled={i === files.length - 1} onclick={() => move(i, 1)}><ArrowDown size={14} /></button>
      </div>
      <button class="icon-btn remove" aria-label="Retirer" onclick={() => onfiles(files.filter((_, j) => j !== i))}><X size={15} /></button>
    </div>
  {/each}
</div>
