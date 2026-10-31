<script lang="ts">
  import { onMount } from "svelte";

  type SelectOption = { value: string | number; label: string };

  let {
    label,
    options,
    value = $bindable<string | number>(),
  } = $props<{ label: string; options: SelectOption[]; value?: string | number }>();

  let isOpen = $state(false);
  let container: HTMLDivElement;
  let trigger: HTMLButtonElement;

  const selectedLabel = $derived(options.find((option) => option.value === value)?.label ?? "Choisir…");

  onMount(() => {
    const closeOnOutsideClick = (event: PointerEvent) => {
      if (container && !container.contains(event.target as Node)) isOpen = false;
    };

    document.addEventListener("pointerdown", closeOnOutsideClick);
    return () => document.removeEventListener("pointerdown", closeOnOutsideClick);
  });

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      isOpen = false;
      trigger?.focus();
      return;
    }
    if (event.key === "ArrowDown" && !isOpen) {
      event.preventDefault();
      isOpen = true;
      setTimeout(() => container?.querySelector<HTMLButtonElement>("[role='option']")?.focus());
      return;
    }
    if (isOpen && ["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      event.preventDefault();
      const options = [...container.querySelectorAll<HTMLButtonElement>("[role='option']")];
      const currentIndex = options.indexOf(document.activeElement as HTMLButtonElement);
      const nextIndex = event.key === "Home"
        ? 0
        : event.key === "End"
          ? options.length - 1
          : (currentIndex + (event.key === "ArrowDown" ? 1 : -1) + options.length) % options.length;
      options[nextIndex]?.focus();
    }
  }
</script>

<div class="select-field">
  <span class="select-label">{label}</span>
  <div class="custom-select" role="group" aria-label={label} bind:this={container}>
    <button
      class="custom-select-trigger"
      bind:this={trigger}
      type="button"
      aria-haspopup="listbox"
      aria-expanded={isOpen}
      aria-label={`${label} : ${selectedLabel}`}
      onclick={() => (isOpen = !isOpen)}
      onkeydown={handleKeydown}
    >
      <span>{selectedLabel}</span>
      <svg class:rotated={isOpen} viewBox="0 0 16 16" aria-hidden="true"><path d="m4 6 4 4 4-4" /></svg>
    </button>
    {#if isOpen}
      <div class="custom-select-options" role="listbox" aria-label={label}>
        {#each options as option (option.value)}
          <button
            type="button"
            role="option"
            aria-selected={value === option.value}
            class:chosen={value === option.value}
            onkeydown={handleKeydown}
            onclick={() => {
              value = option.value;
              isOpen = false;
              trigger?.focus();
            }}
          >
            {option.label}
          </button>
        {/each}
      </div>
    {/if}
  </div>
</div>
