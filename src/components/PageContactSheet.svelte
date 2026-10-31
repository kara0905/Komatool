<script lang="ts">
  type PagePreview = { number: number; width: number; height: number; dataUrl: string };
  let { pages, pagesTruncated }: { pages: PagePreview[]; pagesTruncated: boolean } = $props();
</script>

<section class="contact-sheet" aria-label="Aperçu des pages découpées">
  <div class="contact-sheet-heading">
    <div><strong>Aperçu des pages</strong><span>Vérifiez les coupes avant l’export.</span></div>
    <span class="page-count">{pages.length}{pagesTruncated ? "+" : ""} pages</span>
  </div>
  <div class="page-preview-grid">
    {#each pages as page (page.number)}
      <figure class="page-preview">
        <div class="page-image"><img src={page.dataUrl} alt={`Aperçu de la page ${page.number}`} loading="lazy" decoding="async" /></div>
        <figcaption><strong>Page {String(page.number).padStart(2, "0")}</strong><span>{page.width.toLocaleString("fr-FR")} × {page.height.toLocaleString("fr-FR")}</span></figcaption>
      </figure>
    {/each}
  </div>
  <!-- Le moteur limite les vignettes pour accélérer l'aperçu, pas le nombre de pages exportées. -->
  {#if pagesTruncated}<p class="contact-sheet-note">Affichage limité aux 24 premières pages. Toutes les pages seront bien exportées.</p>{/if}
</section>
