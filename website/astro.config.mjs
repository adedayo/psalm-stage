import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  site: 'https://adedayo.github.io',
  base: '/psalm-stage',
  integrations: [
    starlight({
      title: 'Psalm Stage',
      description:
        'Open-source live worship multitrack player, sample-accurate click, tonic pad player, percussion loop studio, and MIDI stage controller.',
      logo: {
        src: './src/assets/psalm-stage-logo.svg',
      },
      favicon: '/favicon.svg',
      social: [
        {
          icon: 'github',
          label: 'GitHub',
          href: 'https://github.com/adedayo/psalm-stage',
        },
      ],
      customCss: ['./src/styles/custom.css'],
      editLink: {
        baseUrl: 'https://github.com/adedayo/psalm-stage/edit/main/website/',
      },
      sidebar: [
        {
          label: 'Getting Started',
          items: [
            { label: 'Download & Installation', slug: 'getting-started/installation' },
            { label: '5-Minute Sunday Quick Start', slug: 'getting-started/quick-start' },
          ],
        },
        {
          label: 'Stage & Setlist Guide',
          items: [
            { label: 'Live Stage Performance', slug: 'stage-guide/live-stage' },
            { label: 'Setlist Studio & Transitions', slug: 'stage-guide/setlist-studio' },
            { label: 'Tonic Pads & Loop Studio', slug: 'stage-guide/tonic-pads-and-loops' },
          ],
        },
        {
          label: 'Stage Hardware & MIDI',
          items: [
            { label: 'Audio Interfaces & Stage Wiring', slug: 'hardware/audio-routing' },
            { label: 'MIDI Keyboards & Pedalboards', slug: 'hardware/midi-controllers' },
          ],
        },
        {
          label: 'Workbench & Automation',
          items: [
            { label: 'Import Workbench & Song Bundles', slug: 'workbench/import-and-stems' },
            { label: 'AI Setlist Automation (CLI & MCP)', slug: 'workbench/ai-automation' },
          ],
        },
        {
          label: 'Reference',
          items: [
            { label: 'Troubleshooting, Safety & License', slug: 'reference/troubleshooting-and-faq' },
          ],
        },
      ],
    }),
  ],
});
