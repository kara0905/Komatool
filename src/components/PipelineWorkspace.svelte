<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { ArrowDown, ArrowUp, Play, Plus, Save, Trash2 } from "lucide-svelte";
  import ImageQueue from "./ImageQueue.svelte";
  import SelectField from "./SelectField.svelte";
  import AdjustmentsPanel from "./AdjustmentsPanel.svelte";

  type Adjustments = { cropLeft: number; cropTop: number; cropRight: number; cropBottom: number; margin: number; resizeWidth: number | null };
  type Step =
    | { id: string; kind: "upscale"; scale: number; noiseLevel: number; model: string; device: string }
    | { id: string; kind: "convert"; format: string; quality: number }
    | { id: string; kind: "optimize"; lossless: boolean; quality: number }
    | { id: string; kind: "split"; height: number; mode: string; png: boolean; sameSize: boolean; rows: number; columns: number; adjustments: Adjustments }
    | { id: string; kind: "merge"; format: string; adjustments: Adjustments }
    | { id: string; kind: "export"; format: string; includeComicInfo: boolean; title: string; series: string; number: string; writer: string; summary: string; year: string };
  type SavedPipeline = { name: string; steps: Step[]; separateStepFolders?: boolean };

  let { files = $bindable() } = $props<{ files: string[] }>();
  let name = $state("Mon pipeline");
  let steps = $state<Step[]>([makeStep("upscale")]);
  let output = $state("");
  let separateStepFolders = $state(false);
  let saved = $state<SavedPipeline[]>([]);
  let selectedSaved = $state("");
  let busy = $state(false);
  let progress = $state(0);
  let status = $state("Prêt à lancer");
  let error = $state("");
  let message = $state("");

  const tools = [
    { value: "upscale", label: "Upscale / débruitage" },
    { value: "convert", label: "Conversion" },
    { value: "optimize", label: "Optimisation" },
    { value: "split", label: "Découpe" },
    { value: "merge", label: "Fusion" },
    { value: "export", label: "Export CBZ / PDF" },
  ];
  let toolToAdd = $state("upscale");

  function makeStep(kind: string): Step {
    const id = crypto.randomUUID();
    const adjustments: Adjustments = { cropLeft: 0, cropTop: 0, cropRight: 0, cropBottom: 0, margin: 0, resizeWidth: null };
    if (kind === "convert") return { id, kind, format: "png", quality: 82 };
    if (kind === "optimize") return { id, kind, lossless: true, quality: 82 };
    if (kind === "split") return { id, kind, height: 5000, mode: "fixed", png: false, sameSize: false, rows: 2, columns: 2, adjustments };
    if (kind === "merge") return { id, kind, format: "png", adjustments };
    if (kind === "export") return { id, kind, format: "cbz", includeComicInfo: false, title: "", series: "", number: "", writer: "", summary: "", year: "" };
    return { id, kind: "upscale", scale: 2, noiseLevel: -1, model: "cunet", device: "auto" };
  }

  onMount(() => {
    try {
      const value = localStorage.getItem("komatool:pipelines");
      if (value) {
        const parsed: unknown = JSON.parse(value);
        if (Array.isArray(parsed)) {
          saved = parsed.filter((item): item is SavedPipeline =>
            typeof item?.name === "string" && Array.isArray(item.steps),
          );
        }
      }
    } catch {
      saved = [];
    }
    let stop: (() => void) | undefined;
    let disposed = false;
    listen<{ percent: number; message: string }>("task-progress", (event) => {
      progress = event.payload.percent;
      status = event.payload.message;
    }).then((unlisten) => {
      if (disposed) unlisten();
      else stop = unlisten;
    });
    return () => { disposed = true; stop?.(); };
  });

  function persist(next: SavedPipeline[]) {
    localStorage.setItem("komatool:pipelines", JSON.stringify(next));
    saved = next;
  }

  function savePipeline() {
    error = "";
    if (!name.trim() || steps.length === 0) {
      error = "Donne un nom au pipeline et ajoute au moins une étape.";
      return;
    }
    const next = [...saved];
    const existing = next.findIndex((pipeline) => pipeline.name === name.trim());
    const pipeline = { name: name.trim(), steps: JSON.parse(JSON.stringify(steps)) as Step[], separateStepFolders };
    if (existing >= 0) next[existing] = pipeline;
    else next.push(pipeline);
    try {
      persist(next);
      selectedSaved = String(existing >= 0 ? existing : next.length - 1);
      message = "Configuration enregistrée sur cet appareil.";
    } catch {
      error = "Impossible d’enregistrer la configuration dans les préférences de l’application.";
    }
  }

  function loadPipeline() {
    if (!selectedSaved) return;
    const pipeline = saved[Number(selectedSaved)];
    if (!pipeline) return;
    name = pipeline.name;
    steps = JSON.parse(JSON.stringify(pipeline.steps)) as Step[];
    separateStepFolders = pipeline.separateStepFolders ?? false;
    message = `Configuration « ${pipeline.name} » chargée.`;
    error = "";
  }

  function deletePipeline() {
    if (!selectedSaved) return;
    const index = Number(selectedSaved);
    if (!saved[index]) return;
    try {
      persist(saved.filter((_, itemIndex) => itemIndex !== index));
      selectedSaved = "";
    } catch {
      error = "Impossible de supprimer cette configuration.";
    }
  }

  async function addImages() {
    try {
      const result = await open({ multiple: true, filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff"] }] });
      if (result) files = [...files, ...result.filter((path): path is string => typeof path === "string" && !files.includes(path))];
    } catch (cause) { error = String(cause); }
  }

  async function chooseOutput() {
    try {
      const result = await open({ directory: true, multiple: false, title: "Choisir le dossier des résultats" });
      if (typeof result === "string") output = result;
    } catch (cause) { error = String(cause); }
  }

  function moveStep(index: number, offset: number) {
    const destination = index + offset;
    if (destination < 0 || destination >= steps.length) return;
    const next = [...steps];
    [next[index], next[destination]] = [next[destination], next[index]];
    steps = next;
  }

  async function run() {
    error = "";
    message = "";
    if (!files.length) { error = "Ajoute au moins une image au pipeline."; return; }
    if (!output) { error = "Choisis un dossier de sortie."; return; }
    if (!steps.length) { error = "Ajoute au moins une étape."; return; }
    if (steps.slice(0, -1).some((step) => step.kind === "export")) {
      error = "L’export doit être la dernière étape.";
      return;
    }
    busy = true;
    progress = 0;
    status = "Préparation du pipeline…";
    try {
      const pipelineSteps = steps.map(({ id: _id, ...step }) => step);
      message = await invoke<string>("run_pipeline", {
        files,
        output,
        pipeline: { name: name.trim() || "pipeline", steps: pipelineSteps, separateStepFolders },
      });
      progress = 100;
      status = message;
    } catch (cause) {
      error = String(cause);
      status = "Le pipeline a échoué";
    } finally {
      busy = false;
    }
  }
</script>

<div class="pipeline-layout">
  {#if saved.length}
    <div class="pipeline-saved-toolbar">
      <SelectField label="Configurations enregistrées" bind:value={selectedSaved} options={saved.map((item, index) => ({ value: String(index), label: item.name }))} />
      <button class="icon-btn" aria-label="Charger la configuration" title="Charger" onclick={loadPipeline}><ArrowDown size={15} /></button>
      <button class="icon-btn remove" aria-label="Supprimer la configuration" title="Supprimer" onclick={deletePipeline}><Trash2 size={15} /></button>
    </div>
  {/if}

  <section class="panel pipeline-builder">
    <div class="panel-head">
      <div class="step">01</div>
      <div><h2>Construire le pipeline</h2><p>Ajoute les opérations et règle chacune d’elles.</p></div>
    </div>
    <label class="pipeline-name">Nom de la configuration<input bind:value={name} maxlength="48" placeholder="Ex. Préparer un chapitre" /></label>

    <div class="pipeline-steps">
      {#each steps as step, index (step.id)}
        <article class="pipeline-step-card">
          <header class="pipeline-step-title">
            <span class="pipeline-step-number">{String(index + 1).padStart(2, "0")}</span>
            <strong>{tools.find((tool) => tool.value === step.kind)?.label}</strong>
            <div class="reorder">
              <button aria-label="Monter l’étape" disabled={index === 0} onclick={() => moveStep(index, -1)}><ArrowUp size={14} /></button>
              <button aria-label="Descendre l’étape" disabled={index === steps.length - 1} onclick={() => moveStep(index, 1)}><ArrowDown size={14} /></button>
            </div>
            <button class="icon-btn remove" aria-label="Retirer l’étape" onclick={() => steps = steps.filter((item) => item.id !== step.id)}><Trash2 size={14} /></button>
          </header>

          {#if step.kind === "upscale"}
            <div class="pipeline-settings-row"><span>Agrandissement</span><div class="choice-group"><button class:chosen={step.scale === 1} onclick={() => step.scale = 1}>×1</button><button class:chosen={step.scale === 2} onclick={() => step.scale = 2}>×2</button><button class:chosen={step.scale === 4} onclick={() => step.scale = 4}>×4</button></div></div>
            <SelectField label="Réduction du bruit" bind:value={step.noiseLevel} options={[{ value: -1, label: "Désactivée" }, { value: 0, label: "Légère" }, { value: 1, label: "Faible" }, { value: 2, label: "Moyenne" }, { value: 3, label: "Forte" }]} />
            <SelectField label="Modèle" bind:value={step.model} options={[{ value: "cunet", label: "CUNet · détails fins" }, { value: "anime", label: "Anime Art · plus rapide" }]} />
            <div class="pipeline-settings-row"><span>Calcul</span><div class="choice-group"><button class:chosen={step.device === "auto"} onclick={() => step.device = "auto"}>GPU auto</button><button class:chosen={step.device === "cpu"} onclick={() => step.device = "cpu"}>CPU</button></div></div>
          {:else if step.kind === "convert"}
            <SelectField label="Format cible" bind:value={step.format} options={[{ value: "png", label: "PNG · sans perte" }, { value: "jpg", label: "JPEG" }, { value: "webp", label: "WebP" }]} />
            {#if step.format === "jpg"}<label class="pipeline-range">Qualité JPEG <b>{step.quality}%</b><input type="range" min="40" max="100" bind:value={step.quality} /></label>{/if}
          {:else if step.kind === "optimize"}
            <div class="choice-group pipeline-compression"><button class:chosen={step.lossless} onclick={() => step.lossless = true}>Sans perte · PNG</button><button class:chosen={!step.lossless} onclick={() => step.lossless = false}>Avec perte · JPEG</button></div>
            {#if !step.lossless}<label class="pipeline-range">Qualité JPEG <b>{step.quality}%</b><input type="range" min="40" max="100" bind:value={step.quality} /></label>{/if}
          {:else if step.kind === "split"}
            <div class="pipeline-fields"><label>Hauteur cible (px)<input type="number" min="100" max="1000000" bind:value={step.height} /></label><SelectField label="Mode" bind:value={step.mode} options={[{ value: "fixed", label: "Hauteur fixe" }, { value: "smart", label: "Séparations intelligentes" }, { value: "grid", label: "Grille régulière" }]} /></div>
            {#if step.mode === "grid"}<div class="pipeline-fields pipeline-grid-fields"><label>Lignes<input type="number" min="1" max="100" bind:value={step.rows} /></label><label>Colonnes<input type="number" min="1" max="100" bind:value={step.columns} /></label></div>{/if}
            <div class="choice-group pipeline-compression"><button class:chosen={!step.png} onclick={() => step.png = false}>JPG · léger</button><button class:chosen={step.png} onclick={() => step.png = true}>PNG · sans perte</button></div>
            <label class="pipeline-check"><input type="checkbox" bind:checked={step.sameSize} disabled={step.mode !== "fixed"} /> Pages de même hauteur</label>
            <details class="pipeline-advanced"><summary>Recadrage, marges et redimensionnement</summary><AdjustmentsPanel value={step.adjustments} onchange={(value) => step.adjustments = value} /></details>
          {:else if step.kind === "merge"}
            <SelectField label="Format de la fusion" bind:value={step.format} options={[{ value: "png", label: "PNG" }, { value: "jpg", label: "JPG" }, { value: "webp", label: "WebP" }]} />
            <details class="pipeline-advanced"><summary>Recadrage, marges et redimensionnement</summary><AdjustmentsPanel value={step.adjustments} onchange={(value) => step.adjustments = value} /></details>
          {:else}
            <SelectField label="Format d’export" bind:value={step.format} options={[{ value: "cbz", label: "CBZ · bande dessinée" }, { value: "pdf", label: "PDF · document" }]} />
            <label class="pipeline-check"><input type="checkbox" bind:checked={step.includeComicInfo} disabled={step.format !== "cbz"} /> Ajouter ComicInfo.xml</label>
            {#if step.includeComicInfo && step.format === "cbz"}
              <div class="pipeline-metadata-fields">
                <label>Titre<input bind:value={step.title} /></label><label>Série<input bind:value={step.series} /></label>
                <label>Numéro<input bind:value={step.number} /></label><label>Scénariste<input bind:value={step.writer} /></label>
                <label>Année<input bind:value={step.year} maxlength="4" /></label><label>Résumé<input bind:value={step.summary} /></label>
              </div>
            {/if}
            <p class="pipeline-hint">L’export doit être la dernière étape.</p>
          {/if}
        </article>
      {/each}
    </div>

    <div class="pipeline-add-row">
      <SelectField label="Nouvelle étape" bind:value={toolToAdd} options={tools} />
      <button class="secondary-btn" disabled={steps.length >= 20} onclick={() => steps = [...steps, makeStep(toolToAdd)]}><Plus size={15} /> Ajouter</button>
    </div>
    <button class="secondary-btn pipeline-save" onclick={savePipeline}><Save size={15} /> Enregistrer la configuration</button>
  </section>

  <section class="panel pipeline-inputs">
    <div class="panel-head"><div class="step">02</div><div><h2>Entrée et sortie</h2><p>Chaque étape traite le lot produit par la précédente.</p></div></div>
    <div class="file-actions"><button class="secondary-btn" onclick={addImages}><Plus size={15} /> Ajouter des images</button>{#if files.length}<button class="text-btn" onclick={() => files = []}><Trash2 size={14} /> Vider</button>{/if}<span class="count">{files.length} image{files.length === 1 ? "" : "s"}</span></div>
    {#if files.length}<ImageQueue {files} onfiles={(next) => files = next} />{:else}<button class="dropzone" onclick={addImages}><div class="drop-icon"><Plus size={20} /></div><strong>Choisis les images de départ</strong><span>La sortie d’une étape devient l’entrée de la suivante.</span></button>{/if}
    <div class="pipeline-output"><span class="setting-title">Dossier des résultats</span><button class="file-picker" onclick={chooseOutput}><div class="picker-icon"><ArrowDown size={18} /></div><div class="picker-copy"><strong>{output ? output.split(/[\\/]/).pop() : "Choisir un dossier"}</strong><span>{output || "Les résultats finaux seront enregistrés ici."}</span></div></button></div>
    <label class="pipeline-check pipeline-step-folders"><input type="checkbox" bind:checked={separateStepFolders} /> Créer un sous-dossier pour chaque étape</label>
    {#if steps.some((step, index) => step.kind === "export" && index !== steps.length - 1)}<div class="info-note"><span>L’étape d’export doit être placée en dernier.</span></div>{/if}
    {#if error}<div class="error-box">{error}</div>{/if}
    {#if message}<div class="success-box">{message}</div>{/if}
    <div class="pipeline-run-status"><div class="progress-track"><div class="progress-fill" style={`width:${progress}%`}></div></div><span>{status}</span></div>
    <button class="run-button pipeline-run" class:processing={busy} disabled={busy || files.length === 0 || !output || steps.length === 0} onclick={run}>{#if busy}<span class="spinner dark"></span> Traitement en cours…{:else}<Play size={16} /> Lancer le pipeline{/if}</button>
  </section>
</div>
