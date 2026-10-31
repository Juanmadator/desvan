<script lang="ts">
  let { size = 32 }: { size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 1024 1024" aria-hidden="true">
  <defs>
    <linearGradient id="lg-a" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#ffcf6b" /><stop offset="1" stop-color="#f08a24" /></linearGradient>
    <linearGradient id="lg-b" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#6fe3d0" /><stop offset="1" stop-color="#1f9f9a" /></linearGradient>
    <linearGradient id="lg-c" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#a99bff" /><stop offset="1" stop-color="#5b4ad8" /></linearGradient>
    <linearGradient id="lg-d" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#ff8fa3" /><stop offset="1" stop-color="#d9435f" /></linearGradient>
    <linearGradient id="lg-e" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#9be27a" /><stop offset="1" stop-color="#3f9d4a" /></linearGradient>
  </defs>
  <rect x="40" y="40" width="944" height="944" rx="210" fill="#161a24" />
  <g transform="translate(176 176)">
    <rect x="0" y="0" width="408" height="672" rx="40" fill="url(#lg-a)" />
    <rect x="432" y="0" width="240" height="380" rx="40" fill="url(#lg-b)" />
    <rect x="432" y="404" width="240" height="132" rx="34" fill="url(#lg-c)" />
    <rect x="432" y="560" width="128" height="112" rx="30" fill="url(#lg-d)" />
    <rect x="584" y="560" width="88" height="112" rx="26" fill="url(#lg-e)" />
  </g>
</svg>
