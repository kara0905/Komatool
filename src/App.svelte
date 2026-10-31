<script lang="ts">
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { check, type Update } from "@tauri-apps/plugin-updater";
  import { onMount } from "svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import AdjustmentsPanel from "./components/AdjustmentsPanel.svelte";
  import ImageQueue from "./components/ImageQueue.svelte";
  import PageContactSheet from "./components/PageContactSheet.svelte";
  import SelectField from "./components/SelectField.svelte";
  import PipelineWorkspace from "./components/PipelineWorkspace.svelte";
  import {
    Scissors,
    Combine,
    Image as ImageIcon,
    FolderOpen,
    Plus,
    X,
    Play,
    Check,
    Sparkles,
    Trash2,
    Settings2,
    FileImage,
    WandSparkles,
    CircleHelp,
    ArrowUpFromLine,
    Workflow,
    Download,
    RefreshCw,
  } from "lucide-svelte";

  type Mode = "fixed" | "smart" | "grid" | "manual";
  type WorkspaceTab = "split" | "merge" | "combo" | "export" | "upscale" | "convert" | "optimize" | "pipeline";
  type PagePreview = { number: number; width: number; height: number; dataUrl: string };
  type Preview = { width: number; height: number; pages: PagePreview[]; pagesTruncated: boolean };
  type Adjustments = { cropLeft: number; cropTop: number; cropRight: number; cropBottom: number; margin: number; resizeWidth: number | null };
  const defaultAdjustments: Adjustments = { cropLeft: 0, cropTop: 0, cropRight: 0, cropBottom: 0, margin: 0, resizeWidth: null };

  // État partagé par les espaces de travail ; chaque traitement lit les
  // réglages de l'espace actif au moment où l'utilisateur le lance.
  let adjustments = $state<Adjustments>({ ...defaultAdjustments });
  let tab = $state<WorkspaceTab>("split");
  let source = $state("");
  let splitOutput = $state("");
  let mergeOutput = $state("");
  let files = $state<string[]>([]);
  let height = $state(5000);
  let mode = $state<Mode>("fixed");
  let png = $state(false);
  let sameSize = $state(false);
  let busy = $state(false);
  let progress = $state(0);
  let status = $state("Prêt à travailler");
  let error = $state("");
  let done = $state(false);
  let dragActive = $state(false);
  let elapsedSeconds = $state(0);
  let elapsedTimer: number | undefined;
  let isIndeterminate = $derived(busy && /(encodage|enregistrement)/i.test(status));
  let batch = $state(false);
  let rows = $state(2);
  let columns = $state(2);
  let prefix = $state("page_");
  let startNumber = $state(1);
  let exportOutput = $state("");
  let waifuFiles = $state<string[]>([]);
  let waifuOutput = $state("");
  let waifuScale = $state(2);
  let waifuNoise = $state(-1);
  let waifuModel = $state<"cunet" | "anime">("cunet");
  let waifuDevice = $state<"auto" | "cpu">("auto");
  let toolFiles = $state<string[]>([]);
  let pipelineFiles = $state<string[]>([]);
  let toolOutput = $state("");
  let toolFormat = $state("png");
  let toolQuality = $state(82);
  let lossless = $state(true);
  let comicTitle = $state("");
  let comicSeries = $state("");
  let comicNumber = $state("");
  let comicWriter = $state("");
  let comicYear = $state("");
  let comicSummary = $state("");
  let includeComicInfo = $state(false);
  let preview = $state<Preview | null>(null);
  let previewLoading = $state(false);
  let previewRequest = 0;
  let previousPreviewSettings = "";
  let availableUpdate = $state<Update | null>(null);
  let checkingUpdate = $state(false);
  let installingUpdate = $state(false);
  let updateNoticeDismissed = $state(false);
  let updateStatus = $state("");
  let updateProgress = $state(0);
  let updateBytes = 0;
  let updateSize = $state(0);

  // Vérifie les mises à jour sans empêcher l'ouverture de l'espace de travail.
  async function checkForUpdates(showUpToDate = false) {
    if (!isTauri() || checkingUpdate || installingUpdate) return;
    checkingUpdate = true;
    updateStatus = "";
    try {
      const update = await check();
      if (update) {
        if (availableUpdate) await availableUpdate.close();
        availableUpdate = update;
        updateNoticeDismissed = false;
        updateStatus = `Version ${update.version} disponible`;
      } else {
        if (availableUpdate) await availableUpdate.close();
        availableUpdate = null;
        updateStatus = showUpToDate ? "KomaTool est à jour" : "";
        if (showUpToDate) window.setTimeout(() => (updateStatus = ""), 4000);
      }
    } catch (cause) {
      updateStatus = showUpToDate ? `Vérification impossible : ${String(cause)}` : "";
    } finally {
      checkingUpdate = false;
    }
  }

  // Télécharge l'artefact signé et affiche la progression reçue de Tauri.
  async function installAvailableUpdate() {
    if (!availableUpdate || installingUpdate) return;
    installingUpdate = true;
    updateProgress = 0;
    updateBytes = 0;
    updateSize = 0;
    updateStatus = "Téléchargement de la mise à jour…";
    try {
      await availableUpdate.downloadAndInstall((event) => {
        if (event.event === "Started") {
          updateSize = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          updateBytes += event.data.chunkLength;
          if (updateSize > 0) updateProgress = Math.min(100, Math.round((updateBytes / updateSize) * 100));
        } else if (event.event === "Finished") {
          updateProgress = 100;
          updateStatus = "Installation et redémarrage…";
        }
      });
      await relaunch();
    } catch (cause) {
      updateStatus = `Installation impossible : ${String(cause)}`;
      installingUpdate = false;
    }
  }

  async function dismissAvailableUpdate() {
    if (availableUpdate) await availableUpdate.close();
    availableUpdate = null;
    updateNoticeDismissed = true;
    updateStatus = "";
  }

  function reportError(cause: unknown) {
    error = String(cause);
    done = false;
  }

  onMount(() => {
    void checkForUpdates();
    if (navigator.platform.toLowerCase().includes("mac")) {
      document.documentElement.classList.add("macos");
    }
    let stop: (() => void) | undefined;
    let stopDrop: (() => void) | undefined;
    let disposed = false;
    // Les événements natifs Tauri transmettent la progression et les chemins
    // déposés sans convertir les fichiers en données dans le navigateur.
    listen<{ percent: number; message: string }>("task-progress", (event) => {
      progress = event.payload.percent;
      status = event.payload.message;
    }).then((fn) => {
      if (disposed) fn();
      else stop = fn;
    });
    getCurrentWebview().onDragDropEvent(async (event) => {
      if (event.payload.type === "enter" || event.payload.type === "over") dragActive = true;
      else if (event.payload.type === "leave") dragActive = false;
      else if (event.payload.type === "drop") {
        dragActive = false;
        const paths = event.payload.paths;
        const imagePaths: string[] = [];
        for (const path of paths) {
          if (/\.(png|jpe?g|webp|bmp|tiff?)$/i.test(path)) imagePaths.push(path);
          // Un dossier déposé est développé côté Rust en liste d'images triée.
          else {
            try { imagePaths.push(...await invoke<string[]>("list_images", { folder: path })); } catch { /* Ignore les fichiers et dossiers incompatibles. */ }
          }
        }
        const unique = (existing: string[]) => [...existing, ...imagePaths.filter((path) => !existing.includes(path))];
        if (tab === "split") {
          if (batch) files = unique(files);
          else if (imagePaths[0]) source = imagePaths[0];
        } else if (tab === "upscale") waifuFiles = unique(waifuFiles);
        else if (tab === "convert" || tab === "optimize") toolFiles = unique(toolFiles);
        else if (tab === "pipeline") pipelineFiles = unique(pipelineFiles);
        else files = unique(files);
      }
    }).then((fn) => { if (disposed) fn(); else stopDrop = fn; });
    // Détache les écouteurs natifs et le minuteur si le composant est démonté.
    return () => { disposed = true; stop?.(); stopDrop?.(); if (elapsedTimer) window.clearInterval(elapsedTimer); };
  });

  $effect(() => {
    const currentSource = source;
    const currentHeight = height;
    const currentMode = mode;
    const currentSameSize = sameSize;
    const currentRows = rows;
    const currentColumns = columns;
    const currentAdjustments = { ...adjustments };
    const settings = JSON.stringify([currentSource, currentHeight, currentMode, currentSameSize, currentRows, currentColumns, currentAdjustments]);
    if (settings !== previousPreviewSettings) {
      previousPreviewSettings = settings;
    }
    const requestId = ++previewRequest;
    if (!currentSource) { preview = null; previewLoading = false; return; }
    previewLoading = true;
    // Le délai regroupe les changements rapprochés ; l'identifiant empêche
    // une réponse lente de remplacer l'aperçu correspondant aux réglages actuels.
    const timer = window.setTimeout(async () => {
      try {
        const result = await invoke<Preview>("preview_split", {
          source: currentSource, height: currentHeight, mode: currentMode, sameSize: currentSameSize,
          rows: currentRows, columns: currentColumns, adjustments: currentAdjustments,
        });
        if (requestId === previewRequest) preview = result;
      } catch {
        if (requestId === previewRequest) preview = null;
      } finally {
        if (requestId === previewRequest) previewLoading = false;
      }
    }, 220);
    return () => window.clearTimeout(timer);
  });

  async function pickSource() {
    try {
      const result = await open({
        multiple: false,
        filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff"] }],
      });
      if (typeof result === "string") source = result;
    } catch (e) { reportError(e); }
  }
  async function pickOutput() {
    try {
      const result = await open({ directory: true, multiple: false, title: "Choisir le dossier de sortie" });
      if (typeof result === "string") splitOutput = result;
    } catch (e) { reportError(e); }
  }
  async function addFiles() {
    try {
      const result = await open({
        multiple: true,
        filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff"] }],
      });
      if (result) files = [...files, ...result.filter((p): p is string => typeof p === "string" && !files.includes(p))];
    } catch (e) { reportError(e); }
  }
  async function addWaifuFiles() {
    try {
      const result = await open({
        multiple: true,
        filters: [{ name: "Images compatibles Waifu2x", extensions: ["png", "jpg", "jpeg", "webp"] }],
      });
      if (result) waifuFiles = [...waifuFiles, ...result.filter((path): path is string => typeof path === "string" && !waifuFiles.includes(path))];
    } catch (e) { reportError(e); }
  }
  async function addToolFiles() {
    try {
      const result = await open({ multiple: true, filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff"] }] });
      if (result) toolFiles = [...toolFiles, ...result.filter((p): p is string => typeof p === "string" && !toolFiles.includes(p))];
    } catch (e) { reportError(e); }
  }
  async function pickToolOutput() {
    try {
      const result = await open({ directory: true, multiple: false, title: "Choisir le dossier de sortie" });
      if (typeof result === "string") toolOutput = result;
    } catch (e) { reportError(e); }
  }
  async function addFolder() {
    const result = await open({
      directory: true,
      multiple: false,
      title: "Choisir un dossier d’images",
    });
    if (typeof result === "string") {
      try {
        const found = await invoke<string[]>("list_images", { folder: result });
        files = [...files, ...found.filter((p) => !files.includes(p))];
      } catch (e) {
        reportError(e);
      }
    }
  }
  async function pickMergeOutput() {
    try {
      const result = await save({
        title: "Enregistrer l’image fusionnée",
        defaultPath: "fusion.png",
        filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }],
      });
      if (result) mergeOutput = result;
    } catch (e) { reportError(e); }
  }
  async function pickExportOutput() {
    try {
      const result = await save({ title: "Exporter pour la lecture", defaultPath: "lecture.cbz", filters: includeComicInfo ? [{ name: "CBZ", extensions: ["cbz"] }] : [{ name: "CBZ", extensions: ["cbz"] }, { name: "PDF", extensions: ["pdf"] }] });
      if (result) exportOutput = result;
    } catch (e) { reportError(e); }
  }
  async function pickWaifuOutput() {
    try {
      const result = await open({ directory: true, multiple: false, title: "Choisir le dossier des images améliorées" });
      if (typeof result === "string") waifuOutput = result;
    } catch (e) { reportError(e); }
  }
  function name(path: string) {
    return path.split(/[\\/]/).pop() || path;
  }
  async function runTask() {
    error = "";
    done = false;
    progress = 0;
    if (tab === "upscale" && waifuFiles.length < 1) {
      error = "Ajoute au moins une image à améliorer.";
      return;
    }
    if (["convert", "optimize"].includes(tab) && toolFiles.length === 0) { error = "Ajoute au moins une image."; return; }
    if (["convert", "optimize"].includes(tab) && !toolOutput) { error = "Choisis un dossier de sortie."; return; }
    const destination = ["convert", "optimize"].includes(tab) ? toolOutput : tab === "upscale" ? waifuOutput : tab === "merge" ? mergeOutput : tab === "export" ? exportOutput : splitOutput;
    if (!destination) {
      error = tab === "merge" || tab === "export" ? "Choisis un fichier de sortie." : "Choisis un dossier de sortie.";
      return;
    }
    if (tab === "export" && includeComicInfo && !destination.toLowerCase().endsWith(".cbz")) { error = "ComicInfo.xml ne peut être ajouté qu’à un fichier CBZ. Choisis une sortie .cbz."; return; }
    if (!["merge", "export", "upscale", "convert", "optimize"].includes(tab) && mode !== "grid" && (!Number.isInteger(height) || height < 100 || height > 1_000_000)) {
      error = "La hauteur doit être comprise entre 100 et 1 000 000 pixels.";
      return;
    }
    if (tab === "split" && !batch && !source) {
      error = "Choisis une image à découper.";
      return;
    }
    if (!["upscale", "convert", "optimize"].includes(tab) && (tab !== "split" || batch) && files.length < (tab === "export" ? 1 : 2)) {
      error = tab === "export" ? "Ajoute au moins une image." : "Ajoute au moins deux images.";
      return;
    }
    if (!["upscale", "convert", "optimize"].includes(tab) && mode === "smart" && sameSize) {
      error =
        "Le découpage intelligent et les pages de même taille ne peuvent pas être utilisés ensemble.";
      return;
    }
    busy = true;
    status = "Préparation…";
    elapsedSeconds = 0;
    if (elapsedTimer) window.clearInterval(elapsedTimer);
    const taskStarted = Date.now();
    elapsedTimer = window.setInterval(() => { elapsedSeconds = Math.floor((Date.now() - taskStarted) / 1000); }, 1000);
    try {
      let message = "";
      // Chaque espace appelle une commande native dédiée ; les opérations
      // longues restent hors du fil d'affichage dans le moteur Rust.
      if (tab === "upscale") {
        message = await invoke<string>("waifu2x_process", {
          files: waifuFiles,
          output: destination,
          scale: waifuScale,
          noiseLevel: waifuNoise,
          model: waifuModel,
          device: waifuDevice,
        });
      } else if (tab === "convert" || tab === "optimize") {
        const format = tab === "optimize" ? (lossless ? "png" : "jpg") : toolFormat;
        message = await invoke<string>("convert_images", { files: toolFiles, output: destination, format, quality: toolQuality });
      } else if (tab === "split") {
        const requestFor = (item: string) => ({ source: item, output: destination, height, mode, png, sameSize, rows, columns, prefix, startNumber, adjustments });
        message = batch
          ? await invoke<string>("batch_split", { requests: files.map(requestFor) })
          : await invoke<string>("split_advanced", { request: requestFor(source) });
      }
      else if (tab === "merge") {
        if (!destination.toLowerCase().match(/\.(png|jpe?g|webp)$/)) {
          error =
            "Pour une fusion, indique un chemin de fichier PNG, JPG ou WebP.";
          busy = false;
          return;
        }
        message = await invoke<string>("merge_advanced", { files, output: destination, adjustments });
      } else if (tab === "export") {
        message = includeComicInfo
          ? await invoke<string>("export_cbz_metadata", { files, output: destination, title: comicTitle, series: comicSeries, number: comicNumber, writer: comicWriter, year: comicYear, summary: comicSummary })
          : await invoke<string>("export_reading", { files, output: destination });
      } else
        message = await invoke<string>("merge_and_split", {
          files,
          output: destination,
          height,
          mode,
          png,
          sameSize,
          adjustments,
        });
      progress = 100;
      status = message;
      done = true;
    } catch (e) {
      reportError(e);
      status = "Une erreur est survenue";
    } finally {
      busy = false;
      if (elapsedTimer) window.clearInterval(elapsedTimer);
    }
  }
</script>

<svelte:head
  ><title>KomaTool</title><meta
    name="description"
    content="Fusion et découpe d'images pour vos planches."
  /></svelte:head
>

<div class="app-shell" class:drag-active={dragActive}>
  {#if dragActive}<div class="drop-overlay"><div><Plus size={28} /><strong>Déposez vos images</strong><span>Elles seront ajoutées à l’espace de travail actif.</span></div></div>{/if}
  <aside class="sidebar">
    <div class="brand">
      <img class="brand-logo" src="/logo.png" alt="" draggable="false" />
      <div class="brand-name"><strong>KomaTool</strong></div>
    </div>
    <div class="side-label">ESPACE DE TRAVAIL</div>
    <button
      class:active={tab === "split"}
      class="nav-item"
      onclick={() => (tab = "split")}
      ><Scissors size={17} /> Découpe
      <span class="nav-shortcut">01</span></button
    >
    <button
      class:active={tab === "merge"}
      class="nav-item"
      onclick={() => (tab = "merge")}
      ><Combine size={17} /> Fusion <span class="nav-shortcut">02</span></button
    >
    <button
      class:active={tab === "combo"}
      class="nav-item"
      onclick={() => { tab = "combo"; if (mode === "grid" || mode === "manual") mode = "fixed"; }}
      ><WandSparkles size={17} /> Fusion & redécoupe
      <span class="nav-shortcut">03</span></button
    >
    <button class:active={tab === "export"} class="nav-item" onclick={() => (tab = "export")}><FileImage size={17} /> Export lecture <span class="nav-shortcut">04</span></button>
    <button class:active={tab === "upscale"} class="nav-item" onclick={() => (tab = "upscale")}><ArrowUpFromLine size={17} /> Upscale <span class="nav-shortcut">05</span></button>
    <button class:active={tab === "convert"} class="nav-item" onclick={() => (tab = "convert")}><ImageIcon size={17} /> Conversion <span class="nav-shortcut">06</span></button>
    <button class:active={tab === "optimize"} class="nav-item" onclick={() => (tab = "optimize")}><Settings2 size={17} /> Optimisation <span class="nav-shortcut">07</span></button>
    <button class:active={tab === "pipeline"} class="nav-item" onclick={() => (tab = "pipeline")}><Workflow size={17} /> Pipelines <span class="nav-shortcut">08</span></button>
    <div class="sidebar-bottom">
      <div class="local-pill"><span></span> Traitement local</div>
      <div class="version">KOMATOOL <span>v1</span></div>
      <button class="update-check" onclick={() => checkForUpdates(true)} disabled={checkingUpdate || installingUpdate}>
        <span class:spin={checkingUpdate}><RefreshCw size={11} /></span>
        {checkingUpdate ? "Recherche…" : "Vérifier les mises à jour"}
      </button>
    </div>
  </aside>

  <main class="main">
    <header class="topbar" data-tauri-drag-region>
      <div class="breadcrumb" data-tauri-drag-region>
        Outils <span>/</span>
        <b
          >{tab === "split"
            ? "Découpe"
            : tab === "merge"
              ? "Fusion"
              : tab === "export" ? "Export lecture" : tab === "upscale" ? "Upscale" : tab === "convert" ? "Conversion de format" : tab === "optimize" ? "Optimisation du poids" : "Fusion & redécoupe"}</b
        >
      </div>
      <div class="top-right">
        <div class="privacy">
          <span class="privacy-dot"></span> Vos fichiers restent sur votre appareil
        </div>
      </div>
    </header>
    {#if availableUpdate && !updateNoticeDismissed}
      <div class="update-banner" role="status">
        <div class="update-banner-icon"><Download size={17} /></div>
        <div class="update-banner-copy">
          <strong>KomaTool {availableUpdate.version} est disponible</strong>
          <span>{installingUpdate ? (updateSize ? `Téléchargement · ${updateProgress}%` : updateStatus) : updateStatus.startsWith("Installation impossible") ? updateStatus : "La mise à jour sera installée puis l’application redémarrée."}</span>
          {#if installingUpdate && updateSize > 0}
            <div class="update-progress"><span style={`width:${updateProgress}%`}></span></div>
          {/if}
        </div>
        <button class="update-install" onclick={installAvailableUpdate} disabled={installingUpdate}>
          {installingUpdate ? "Installation…" : "Installer"}
        </button>
        {#if !installingUpdate}
          <button class="update-later" onclick={dismissAvailableUpdate}>Plus tard</button>
        {/if}
      </div>
    {/if}
    {#if updateStatus && !availableUpdate && !checkingUpdate}
      <div class="update-toast" role="status">{updateStatus}</div>
    {/if}
    <section class="workspace">
      <div class="page-heading">
        <div>
          <div class="eyebrow">
            ATELIER IMAGE <span>•</span>
            {tab === "split" ? "01" : tab === "merge" ? "02" : tab === "combo" ? "03" : tab === "export" ? "04" : tab === "upscale" ? "05" : tab === "convert" ? "06" : tab === "optimize" ? "07" : "08"}
          </div>
          <h1>
            {tab === "split"
              ? "Découper une planche"
              : tab === "merge"
                ? "Fusionner des images"
                : tab === "combo" ? "Fusionner & redécouper" : tab === "export" ? "Préparer un livre d’images" : tab === "upscale" ? "Upscaler des images de BD" : tab === "convert" ? "Convertir des images" : tab === "optimize" ? "Optimiser le poids des images" : "Créer un pipeline d’images"}
          </h1>
          <p>
            {tab === "split"
              ? "Divisez une longue image en pages prêtes à être utilisées."
              : tab === "merge"
                ? "Assemblez plusieurs images verticalement dans leur ordre de lecture."
                : tab === "combo" ? "Assemblez vos images, puis découpez automatiquement le résultat." : tab === "export" ? "Regroupez vos pages dans un fichier CBZ ou PDF, avec leurs métadonnées." : tab === "upscale" ? "Agrandissez et débruitez vos planches avec des modèles Waifu2x pour illustrations." : tab === "convert" ? "Changez le format d’un lot d’images en une seule opération." : tab === "optimize" ? "Réduisez l’espace occupé par vos images et comparez le résultat." : "Enchaînez plusieurs outils avec vos propres réglages et relancez-les facilement."}
          </p>
        </div>
        <div class="heading-icon">
          {#if tab === "pipeline"}<Workflow size={24} />{:else if tab === "split"}<Scissors
              size={24}
            />{:else if tab === "merge"}<Combine
              size={24}
            />{:else if tab === "export"}<FileImage size={24} />{:else if tab === "upscale"}<ArrowUpFromLine size={24} />{:else if tab === "convert" || tab === "optimize"}<ImageIcon size={24} />{:else}<WandSparkles size={24} />{/if}
        </div>
      </div>

      {#if tab === "pipeline"}
        <PipelineWorkspace bind:files={pipelineFiles} />
      {:else}
      <div class="content-grid">
        <div class="left-column">
          {#if tab === "split"}
            <section class="panel">
              <div class="panel-head">
                <div class="step">01</div>
                <div>
                  <h2>Image source</h2>
                  <p>Sélectionnez la planche à traiter.</p>
                </div>
              </div>
              <button class="file-picker" onclick={pickSource}
                ><div class="picker-icon"><ImageIcon size={20} /></div>
                <div class="picker-copy">
                  <strong>{source ? name(source) : "Choisir une image"}</strong
                  ><span>{source || "PNG, JPG, WEBP, TIFF…"}</span>
                </div>
                <span class="picker-action"
                  >Parcourir <FolderOpen size={15} /></span
                ></button
              >
              <label class="check-row batch-toggle"><input type="checkbox" bind:checked={batch} /><span class="fake-check"></span><span><b>Traiter plusieurs images</b><small>Chaque source sera découpée dans un sous-dossier dédié.</small></span></label>
              {#if batch}
                <div class="file-actions"><button class="secondary-btn" onclick={addFiles}><Plus size={16} /> Ajouter les images</button><button class="secondary-btn" onclick={addFolder}><FolderOpen size={16} /> Importer un dossier</button></div>
                {#if files.length}<ImageQueue {files} onfiles={(next) => files = next} />{/if}
              {/if}
            </section>
          {:else if tab === "upscale"}
            <section class="panel">
              <div class="panel-head">
                <div class="step">01</div>
                <div><h2>Images à améliorer</h2><p>Waifu2x accepte les fichiers PNG, JPG et WebP.</p></div>
                <span class="count">{waifuFiles.length} image{waifuFiles.length === 1 ? "" : "s"}</span>
              </div>
              <div class="file-actions">
                <button class="secondary-btn" onclick={addWaifuFiles}><Plus size={16} /> Ajouter des images</button>
                {#if waifuFiles.length}<button class="text-btn" onclick={() => waifuFiles = []}><Trash2 size={14} /> Vider</button>{/if}
              </div>
              {#if waifuFiles.length}<ImageQueue files={waifuFiles} onfiles={(next) => waifuFiles = next} />
              {:else}<button class="dropzone" onclick={addWaifuFiles}><div class="drop-icon"><Plus size={21} /></div><strong>Ajoutez vos planches</strong><span>PNG, JPG ou WebP</span></button>{/if}
            </section>
            <section class="panel">
              <div class="panel-head"><div class="step">02</div><div><h2>Réglages Waifu2x</h2><p>Modèles spécialisés dans les illustrations et les dessins.</p></div></div>
              <div class="field-row"><div><span class="setting-title">Agrandissement</span><span class="field-hint">×1 applique uniquement le débruitage</span></div><div class="choice-group"><button class:chosen={waifuScale === 1} onclick={() => waifuScale = 1}>×1</button><button class:chosen={waifuScale === 2} onclick={() => waifuScale = 2}>×2</button><button class:chosen={waifuScale === 4} onclick={() => waifuScale = 4}>×4</button></div></div>
              <SelectField label="Réduction du bruit" bind:value={waifuNoise} options={[
                { value: -1, label: "Désactivée" },
                { value: 0, label: "Légère" },
                { value: 1, label: "Faible" },
                { value: 2, label: "Moyenne" },
                { value: 3, label: "Forte" },
              ]} />
              <div class="field-row"><div><span class="setting-title">Calcul</span><span class="field-hint">GPU automatique ou processeur</span></div><div class="choice-group"><button class:chosen={waifuDevice === "auto"} onclick={() => waifuDevice = "auto"}>Auto</button><button class:chosen={waifuDevice === "cpu"} onclick={() => waifuDevice = "cpu"}>CPU</button></div></div>
              <div class="model-choices">
                <button class:chosen={waifuModel === "cunet"} class="model-choice" onclick={() => waifuModel = "cunet"}><strong>CUNet</strong><span>Meilleure qualité · détails fins</span></button>
                <button class:chosen={waifuModel === "anime"} class="model-choice" onclick={() => waifuModel = "anime"}><strong>Anime Art</strong><span>Plus rapide · dessin couleur</span></button>
              </div>
            </section>
          {:else if tab === "convert" || tab === "optimize"}
            <section class="panel">
              <div class="panel-head"><div class="step">01</div><div><h2>Images à traiter</h2><p>Ajoutez les fichiers puis vérifiez leur ordre.</p></div><span class="count">{toolFiles.length} image{toolFiles.length === 1 ? "" : "s"}</span></div>
              <div class="file-actions"><button class="secondary-btn" onclick={addToolFiles}><Plus size={16} /> Ajouter des images</button>{#if toolFiles.length}<button class="text-btn" onclick={() => toolFiles = []}><Trash2 size={14} /> Vider</button>{/if}</div>
              {#if toolFiles.length}<ImageQueue files={toolFiles} onfiles={(next) => toolFiles = next} />{:else}<button class="dropzone" onclick={addToolFiles}><div class="drop-icon"><Plus size={21} /></div><strong>Ajoutez vos images ici</strong><span>PNG, JPG, WebP, BMP ou TIFF</span></button>{/if}
            </section>
            <section class="panel"><div class="panel-head"><div class="step">02</div><div><h2>Réglages de sortie</h2><p>{tab === "convert" ? "Choisissez le format de destination." : "Choisissez une compression adaptée à vos images."}</p></div></div>
              {#if tab === "convert"}<SelectField label="Format cible" bind:value={toolFormat} options={[
                { value: "png", label: "PNG · sans perte" },
                { value: "jpg", label: "JPEG · avec perte" },
                { value: "webp", label: "WebP · sans perte" },
              ]} />{/if}
              {#if tab === "optimize"}<div class="field-row"><div><span class="setting-title">Type de compression</span><span class="field-hint">Sans perte conserve chaque pixel</span></div><div class="choice-group"><button class:chosen={lossless} onclick={() => lossless = true}>Sans perte</button><button class:chosen={!lossless} onclick={() => lossless = false}>Avec perte</button></div></div>{/if}
              {#if (tab === "convert" && toolFormat === "jpg") || (tab === "optimize" && !lossless)}<label class="quality-control">Qualité JPEG <b>{toolQuality}%</b><input type="range" min="40" max="100" bind:value={toolQuality} /></label>{/if}
              {#if tab === "optimize"}<div class="info-note"><Sparkles size={16} /><span>{lossless ? "PNG utilise une compression sans perte. Les images déjà compactes peuvent ne pas beaucoup diminuer." : "JPEG réduit davantage le poids, avec une légère perte de qualité réglable."}</span></div>{/if}
            </section>
          {:else}
            <section class="panel">
              <div class="panel-head">
                <div class="step">01</div>
                <div>
                  <h2>{tab === "export" ? "Pages à exporter" : "Images à assembler"}</h2>
                  <p>Ajoutez vos fichiers puis définissez leur ordre.</p>
                </div>
                <span class="count"
                  >{files.length} fichier{files.length > 1 ? "s" : ""}</span
                >
              </div>
              <div class="file-actions">
                <button class="secondary-btn" onclick={addFiles}
                  ><Plus size={16} /> Ajouter des images</button
                ><button class="secondary-btn" onclick={addFolder}
                  ><FolderOpen size={16} /> Importer un dossier</button
                >
              </div>
              {#if files.length}
                <ImageQueue {files} onfiles={(next) => files = next} />
                <button class="text-btn" onclick={() => (files = [])}
                  ><Trash2 size={14} /> Vider la sélection</button
                >
              {:else}<button class="dropzone" onclick={addFiles}
                  ><div class="drop-icon"><Plus size={21} /></div>
                  <strong>Ajoutez vos images ici</strong><span
                    >Parcourir les fichiers ou importer un dossier</span
                  ></button
                >{/if}
            </section>
            {#if tab === "export"}
              <section class="panel"><div class="panel-head"><div class="step">02</div><div><h2>Métadonnées CBZ</h2><p>Optionnel · uniquement pour le format CBZ.</p></div></div>
                <label class="check-row"><input type="checkbox" checked={includeComicInfo} onchange={(event) => { includeComicInfo = event.currentTarget.checked; if (includeComicInfo && exportOutput.toLowerCase().endsWith(".pdf")) exportOutput = ""; }} /><span class="fake-check"></span><span><b>Ajouter ComicInfo.xml</b><small>Ajoute les informations de la série dans l’archive.</small></span></label>
                {#if includeComicInfo}<div class="metadata-grid"><label>Titre du tome<input maxlength="200" bind:value={comicTitle} placeholder="Ex. Le grand départ" /></label><label>Série<input maxlength="200" bind:value={comicSeries} placeholder="Nom de la série" /></label><label>Numéro<input maxlength="40" bind:value={comicNumber} placeholder="1" /></label><label>Scénariste / auteur<input maxlength="200" bind:value={comicWriter} /></label><label>Année<input maxlength="4" bind:value={comicYear} placeholder="2026" /></label><label class="metadata-summary">Résumé<textarea maxlength="4000" rows="4" bind:value={comicSummary}></textarea></label></div>{/if}
              </section>
            {/if}
          {/if}

          {#if tab === "split" || tab === "combo"}
            <section class="panel">
              <div class="panel-head">
                <div class="step">02</div>
                <div>
                  <h2>Paramètres de découpe</h2>
                  <p>Choisissez la façon dont les pages seront séparées.</p>
                </div>
              </div>
              <div class="mode-grid">
                <button
                  class:chosen={mode === "fixed"}
                  class="mode-card"
                  onclick={() => (mode = "fixed")}
                  ><div class="mode-visual fixed-visual">
                    <i></i><i></i><i></i>
                  </div>
                  <strong>Hauteur fixe</strong><span>Découpe régulière</span
                  >{#if mode === "fixed"}<Check
                      size={15}
                      class="chosen-check"
                    />{/if}</button
                ><button
                  class:chosen={mode === "smart"}
                  class="mode-card"
                  onclick={() => {
                    mode = "smart";
                    sameSize = false;
                  }}
                  ><div class="mode-visual smart-visual">
                    <i></i><i></i><i></i>
                  </div>
                  <strong>Intelligente</strong><span
                    >Recherche les séparations</span
                  >{#if mode === "smart"}<Check
                      size={15}
                      class="chosen-check"
                    />{/if}</button
                >
                {#if tab === "split"}<button class:chosen={mode === "grid"} class="mode-card" onclick={() => { mode = "grid"; sameSize = false; }}><div class="mode-visual grid-visual"><i></i><i></i><i></i><i></i></div><strong>Grille</strong><span>Découpe en tuiles</span>{#if mode === "grid"}<Check size={15} class="chosen-check" />{/if}</button>
                <button class:chosen={mode === "manual"} class="mode-card" onclick={() => { mode = "manual"; sameSize = false; }}><div class="mode-visual manual-visual"><i></i><i></i><i></i></div><strong>Manuelle</strong><span>Ajustez les lignes</span>{#if mode === "manual"}<Check size={15} class="chosen-check" />{/if}</button>{/if}
              </div>
              {#if mode === "grid" && tab === "split"}<div class="field-row grid-counts"><label>Lignes <input type="number" min="1" max="100" bind:value={rows} /></label><label>Colonnes <input type="number" min="1" max="100" bind:value={columns} /></label></div>
              {:else}<div class="field-row">
                <div>
                  <label for="height">Hauteur cible</label><span
                    class="field-hint"
                    >En pixels · {mode === "smart"
                      ? "hauteur approximative"
                      : "hauteur de chaque page"}</span
                  >
                </div>
                <div class="number-input">
                  <input
                    id="height"
                    type="number"
                    min="100"
                    max="1000000"
                    step="100"
                    bind:value={height}
                  /><span>px</span>
                </div>
              </div>{/if}
              {#if mode === "fixed"}<label class="check-row"
                  ><input type="checkbox" bind:checked={sameSize} /><span
                    class="fake-check"
                  ></span><span
                    ><b>Uniformiser la hauteur des pages</b><small
                      >Répartit la hauteur totale pour obtenir des pages de
                      taille identique.</small
                    ></span
                  ></label
                >{:else if mode === "smart"}<div class="info-note">
                  <Sparkles size={16} /><span
                    >Recherche une ligne de séparation à proximité de la hauteur
                    cible. Le résultat peut varier selon le contenu de l'image.</span
                  >
                </div>{:else if mode === "manual" && tab === "split"}<div class="info-note"><Sparkles size={16} /><span>Les séparateurs de l’aperçu peuvent être déplacés à la souris.</span></div>{/if}
              <label class="check-row"
                ><input type="checkbox" bind:checked={png} /><span
                  class="fake-check"
                ></span><span
                  ><b>Exporter en PNG</b><small
                    >{png
                      ? "Qualité sans perte, fichiers plus volumineux."
                      : "Export JPG haute qualité, fichiers plus légers."}</small
                  ></span
                ></label
              >
              {#if !png}<div class="info-note jpeg-limit-note"><ImageIcon size={16} /><span>Limite JPEG : chaque page ne peut pas dépasser 65 535 px en largeur ou en hauteur. Utilisez PNG ou réduisez la hauteur cible pour les très grandes planches.</span></div>{/if}
              {#if mode === "grid" && tab === "split"}<div class="info-note"><Sparkles size={16} /><span>La grille produira {rows * columns} tuiles.</span></div>{/if}
              {#if tab === "split" && source}
                <div class="split-preview">
                  {#if previewLoading}
                    <div class="preview-loading" role="status" aria-live="polite"><span class="spinner dark"></span><span>Génération des aperçus…</span></div>
                  {:else if preview}
                    <PageContactSheet pages={preview.pages} pagesTruncated={preview.pagesTruncated} />
                  {/if}
                </div>
              {/if}
              <details class="adjustments"><summary>Recadrage, marges et redimensionnement</summary><AdjustmentsPanel value={adjustments} onchange={(next) => adjustments = next} /></details>
              {#if tab === "split"}<div class="field-row naming-row"><label>Préfixe des fichiers <input maxlength="40" bind:value={prefix} /></label><label>Numéro initial <input type="number" min="0" bind:value={startNumber} /></label></div>{/if}
            </section>
          {/if}

          {#if tab === "merge"}<section class="panel"><div class="panel-head"><div class="step">02</div><div><h2>Options de fusion</h2><p>Réglages appliqués à chaque image avant l’assemblage.</p></div></div><details class="adjustments"><summary>Retouches communes</summary><AdjustmentsPanel value={adjustments} onchange={(next) => adjustments = next} />{#if /\.jpe?g$/i.test(mergeOutput)}<div class="info-note jpeg-limit-note"><ImageIcon size={16} /><span>Limite JPEG : chaque côté de l’image finale doit rester sous 65 535 px. KomaTool signalera les dimensions dépassées avant l’enregistrement.</span></div>{/if}</details></section>{/if}

          <section class="panel">
            <div class="panel-head">
              <div class="step">03</div>
              <div>
                <h2>
                  {tab === "merge" || tab === "export" ? "Fichier de sortie" : "Dossier de sortie"}
                </h2>
                <p>
                  {tab === "merge" || tab === "export"
                    ? "Choisissez le nom et le format du fichier final."
                    : tab === "upscale" ? "Choisissez où enregistrer les images améliorées." : "Choisissez où enregistrer les pages générées."}
                </p>
              </div>
            </div>
            {#if tab === "convert" || tab === "optimize"}<button class="file-picker" onclick={pickToolOutput}><div class="picker-icon"><FolderOpen size={20} /></div><div class="picker-copy"><strong>{toolOutput ? name(toolOutput) : "Choisir un dossier"}</strong><span>{toolOutput || "Les images traitées seront enregistrées ici."}</span></div><span class="picker-action">Parcourir <FolderOpen size={15} /></span></button>
            {:else if tab === "merge"}<button
                class="file-picker"
                onclick={pickMergeOutput}
                ><div class="picker-icon"><ImageIcon size={20} /></div>
                <div class="picker-copy">
                <strong
                    >{mergeOutput
                      ? name(mergeOutput)
                      : "Choisir le fichier final"}</strong
                  ><span>{mergeOutput || "PNG, JPG ou WEBP"}</span>
                </div>
                <span class="picker-action"
                  >Parcourir <FolderOpen size={15} /></span
                ></button
              >
            {:else if tab === "export"}<button class="file-picker" onclick={pickExportOutput}
                ><div class="picker-icon"><FileImage size={20} /></div><div class="picker-copy"><strong>{exportOutput ? name(exportOutput) : "Choisir un fichier CBZ ou PDF"}</strong><span>{exportOutput || "Archive de lecture ou document paginé"}</span></div><span class="picker-action">Parcourir <FolderOpen size={15} /></span></button>
            {:else if tab === "upscale"}<button class="file-picker" onclick={pickWaifuOutput}
                ><div class="picker-icon"><FolderOpen size={20} /></div><div class="picker-copy"><strong>{waifuOutput ? name(waifuOutput) : "Choisir un dossier"}</strong><span>{waifuOutput || "Les images améliorées seront enregistrées en PNG."}</span></div><span class="picker-action">Parcourir <FolderOpen size={15} /></span></button>
            {:else}<button class="file-picker" onclick={pickOutput}
                ><div class="picker-icon"><FolderOpen size={20} /></div>
                <div class="picker-copy">
                  <strong>{splitOutput ? name(splitOutput) : "Choisir un dossier"}</strong
                  ><span
                    >{splitOutput ||
                      "Les images générées seront enregistrées ici."}</span
                  >
                </div>
                <span class="picker-action"
                  >Parcourir <FolderOpen size={15} /></span
                ></button
              >{/if}
          </section>
        </div>

        <aside class="right-column">
          <div class="summary-card">
            <div class="summary-top">
              <div class="summary-icon"><Settings2 size={18} /></div>
              <span>RÉCAPITULATIF</span>
            </div>
            <h3>
              {tab === "split"
                ? "Votre découpe"
                : tab === "merge"
                  ? "Votre fusion"
              : tab === "export" ? "Votre export" : tab === "upscale" ? "Votre upscale" : tab === "convert" ? "Votre conversion" : tab === "optimize" ? "Votre optimisation" : "Votre redécoupe"}
            </h3>
            <p class="summary-desc">
              {tab === "split"
                ? "Une image sera divisée en plusieurs fichiers."
                : tab === "merge"
                  ? "Les images seront assemblées verticalement."
                : tab === "export" ? (includeComicInfo ? "Les pages et ComicInfo.xml seront réunis dans le CBZ." : "Les images seront réunies dans un fichier de lecture.") : tab === "upscale" ? (waifuScale === 1 ? "Waifu2x débruitera les images sans changer leurs dimensions." : `Waifu2x agrandira les images ×${waifuScale} et les débruitera selon le réglage choisi.`) : tab === "convert" ? "Vos images seront réencodées dans le format choisi." : tab === "optimize" ? `Compression ${lossless ? "sans perte (PNG)" : "avec perte (JPEG)"}.` : "Les images seront assemblées puis découpées en pages."}
            </p>
            <div class="summary-divider"></div>
            <div class="summary-line">
              <span>Opération</span><b
                >{tab === "split"
                  ? "Découpe"
                  : tab === "merge"
                    ? "Fusion"
                  : tab === "export" ? (includeComicInfo ? "CBZ + ComicInfo" : "Export CBZ / PDF") : tab === "upscale" ? `Upscale · ×${waifuScale}` : tab === "convert" ? `Conversion · ${toolFormat.toUpperCase()}` : tab === "optimize" ? (lossless ? "Compression PNG" : `JPEG · ${toolQuality}%`) : "Fusion + découpe"}</b
              >
            </div>
            {#if tab !== "split"}<div class="summary-line">
                <span>Images sélectionnées</span><b>{tab === "upscale" ? waifuFiles.length : ["convert", "optimize"].includes(tab) ? toolFiles.length : files.length}</b>
              </div>{/if}
            {#if tab === "convert" || tab === "optimize"}<div class="summary-line"><span>Format</span><b>{tab === "convert" ? toolFormat.toUpperCase() : lossless ? "PNG sans perte" : `JPEG · ${toolQuality}%`}</b></div>{/if}
            {#if tab === "upscale"}
              <div class="summary-line"><span>Modèle</span><b>{waifuModel === "cunet" ? "CUNet" : "Anime Art"}</b></div>
              <div class="summary-line"><span>Débruitage</span><b>{waifuNoise < 0 ? "Non" : `Niveau ${waifuNoise}`}</b></div>
              <div class="summary-line"><span>Calcul</span><b>{waifuDevice === "auto" ? "GPU auto" : "CPU"}</b></div>
            {/if}
            {#if !["merge", "export", "upscale", "convert", "optimize"].includes(tab)}<div class="summary-line">
                <span>Mode</span><b
                  >{mode === "fixed" ? "Hauteur fixe" : mode === "smart" ? "Intelligent" : mode === "grid" ? "Grille" : "Manuel"}</b
                >
              </div>
              <div class="summary-line">
                <span>Hauteur cible</span><b
                  >{height.toLocaleString("fr-FR")} px</b
                >
              </div>
              <div class="summary-line">
                <span>Format</span><b>{png ? "PNG" : "JPG"}</b>
              </div>{/if}
            <div class="summary-divider"></div>
            <button class="run-button" disabled={busy} onclick={runTask}
              >{#if busy}<span class="spinner"></span> Traitement en cours{:else}<Play
                  size={16}
                  fill="currentColor"
                /> {tab === "upscale" ? (waifuScale === 1 ? "Débruiter les images" : "Upscaler les images") : tab === "export" ? "Créer l’export" : tab === "convert" ? "Convertir les images" : tab === "optimize" ? "Optimiser les images" : "Lancer le traitement"}{/if}</button
            >
            <div class="shortcut-hint">
              <CircleHelp size={13} /> {tab === "upscale" || tab === "export" || tab === "convert" || tab === "optimize" ? "Les images seront traitées dans l’ordre affiché." : "Vérifiez l'ordre des images avant de lancer."}
            </div>
          </div>
          <div class="tip-card">
            <div class="tip-icon"><Sparkles size={17} /></div>
            <div>
              <b>Astuce</b>
              <p>
                {tab === "split"
                  ? "Le mode intelligent est particulièrement utile pour les longues planches de webtoon."
                  : tab === "merge"
                    ? "Utilisez les flèches pour réorganiser les images avant la fusion."
                    : tab === "export" ? (includeComicInfo ? "Les champs laissés vides sont ignorés dans ComicInfo.xml." : "L’ordre des pages détermine leur ordre dans le CBZ ou le PDF.") : tab === "upscale" ? "CUNet privilégie les détails fins ; Anime Art traite plus vite les dessins couleur." : tab === "convert" ? "JPEG utilise le réglage de qualité ; PNG et WebP conservent les pixels." : tab === "optimize" ? "Le bilan indique le gain ou la hausse de poids après encodage." : "Le mode intelligent recherche des zones de séparation pour limiter les coupures au milieu du contenu."}
              </p>
            </div>
          </div>
        </aside>
      </div>
      {/if}

      {#if busy || done || error}<section
          class="task-status"
          class:success={done}
          class:failed={!!error}
        >
          <div class="status-head">
            <div class="status-indicator">
              {#if done}<Check size={16} />{:else if error}<X
                  size={16}
                />{:else}<span class="spinner dark"></span>{/if}
            </div>
            <div class="status-copy">
              <b
                >{error
                  ? "Traitement interrompu"
                  : done
                    ? "Opération terminée"
                    : "Traitement en cours"}</b
              ><span>{error || status}{busy && elapsedSeconds > 0 ? ` · ${Math.floor(elapsedSeconds / 60).toString().padStart(2, "0")}:${(elapsedSeconds % 60).toString().padStart(2, "0")} écoulées` : ""}</span>
            </div>
            <strong>{isIndeterminate ? "…" : `${progress}%`}</strong>
          </div>
            <div class="progress-track" class:indeterminate={isIndeterminate}>
            <div class="progress-fill" style={`width:${progress}%`}></div>
          </div>
        </section>{/if}
      <footer>
        KOMATOOL <span>·</span> Un outil simple pour vos planches d'images
      </footer>
    </section>
  </main>
</div>
