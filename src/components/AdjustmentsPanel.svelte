<script lang="ts">
  type Adjustments = { cropLeft: number; cropTop: number; cropRight: number; cropBottom: number; margin: number; resizeWidth: number | null };
  let { value, onchange }: { value: Adjustments; onchange: (next: Adjustments) => void } = $props();
  const fields = [
    ["cropLeft", "Gauche"], ["cropTop", "Haut"],
    ["cropRight", "Droite"], ["cropBottom", "Bas"],
  ] as const;
  function set(key: keyof Adjustments, raw: string) {
    // Un champ de largeur vide désactive le redimensionnement ; les autres
    // valeurs sont converties en nombres positifs avant d'être remontées.
    const next = { ...value, [key]: raw === "" && key === "resizeWidth" ? null : Math.max(0, Number(raw)) };
    onchange(next);
  }
</script>

<div class="adjustments-grid">
  {#each fields as [key, label]}
    <label>{label} <span>px</span><input type="number" min="0" value={value[key]} oninput={(e) => set(key, e.currentTarget.value)} /></label>
  {/each}
  <label>Marge <span>px</span><input type="number" min="0" max="2000" value={value.margin} oninput={(e) => set("margin", e.currentTarget.value)} /></label>
  <label>Largeur finale <span>px · vide = inchangée</span><input type="number" min="1" max="20000" value={value.resizeWidth ?? ""} oninput={(e) => set("resizeWidth", e.currentTarget.value)} /></label>
</div>
