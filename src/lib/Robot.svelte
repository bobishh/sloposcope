<script>
  import { onMount } from 'svelte';

  let { targetX = 0, targetY = 0, active = false } = $props();

  let x = $state(0);
  let y = $state(0);
  let rotation = $state(0);
  let hammerRotation = $state(0);
  let opacity = $state(0.95);

  let rafId;

  function update() {
    const safeTargetX = Number.isFinite(Number(targetX)) ? Number(targetX) : x;
    const safeTargetY = Number.isFinite(Number(targetY)) ? Number(targetY) : y;

    if (!active) {
      opacity = Math.max(0.6, opacity - 0.015);
    } else {
      opacity = Math.min(1, opacity + 0.03);

      const dx = safeTargetX - x;
      const dy = safeTargetY - y;
      const dist = Math.sqrt(dx * dx + dy * dy);

      if (dist > 5) {
        x += dx * 0.09;
        y += dy * 0.09;
        rotation = Math.sin(Date.now() * 0.01) * 10;
        hammerRotation = Math.sin(Date.now() * 0.026) * 62;
      } else {
        rotation *= 0.9;
        hammerRotation = Math.sin(Date.now() * 0.01) * 24;
      }
    }

    rafId = requestAnimationFrame(update);
  }

  onMount(() => {
    x = Number.isFinite(Number(targetX)) ? Number(targetX) : 24;
    y = Number.isFinite(Number(targetY)) ? Number(targetY) : 24;
    rafId = requestAnimationFrame(update);
    return () => cancelAnimationFrame(rafId);
  });
</script>

<g transform="translate({x}, {y}) scale(0.72) rotate({rotation})" style="opacity: {opacity}; pointer-events: none;">
  <defs>
    <linearGradient id="robot-metal" x1="-40" y1="-40" x2="40" y2="40" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#b2f5bf"/>
      <stop offset="1" stop-color="#2d7f5d"/>
    </linearGradient>
    <linearGradient id="hammer-metal" x1="-10" y1="-6" x2="10" y2="6" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#f0f0f0"/>
      <stop offset="1" stop-color="#8f8f8f"/>
    </linearGradient>
  </defs>

  <circle cx="0" cy="0" r="38" fill="rgba(121, 255, 180, 0.15)" stroke="rgba(121, 255, 180, 0.45)" stroke-width="2" />

  <rect x="-16" y="-26" width="32" height="22" rx="6" fill="url(#robot-metal)" stroke="#0e2f22" stroke-width="2"/>
  <rect x="-14" y="-2" width="28" height="24" rx="5" fill="url(#robot-metal)" stroke="#0e2f22" stroke-width="2"/>
  <rect x="-9" y="-18" width="6" height="4" rx="1" fill="#8ef7b5"/>
  <rect x="3" y="-18" width="6" height="4" rx="1" fill="#ff9e9e"/>
  <path d="M-8 -10 L8 -10" stroke="#0d221b" stroke-width="2" stroke-linecap="round"/>

  <rect x="-23" y="0" width="6" height="18" rx="2" fill="#2f5f4a"/>
  <rect x="17" y="0" width="6" height="18" rx="2" fill="#2f5f4a"/>
  <rect x="-10" y="22" width="7" height="12" rx="2" fill="#2f5f4a"/>
  <rect x="3" y="22" width="7" height="12" rx="2" fill="#2f5f4a"/>

  <g transform="translate(21, 10) rotate({hammerRotation})">
    <rect x="-1.5" y="-2" width="3" height="20" rx="1.5" fill="#8b5a2b"/>
    <rect x="-8" y="-7" width="16" height="8" rx="2" fill="url(#hammer-metal)" stroke="#5c5c5c" stroke-width="1"/>
  </g>
</g>
