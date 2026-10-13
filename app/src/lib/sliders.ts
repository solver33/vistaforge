// Labels for the core's sliders (core/src/settings.rs `Sliders`). A slider the core adds later
// shows up under "More" with its key as the label.

export interface SliderMeta {
  key: string;
  label: string;
  help: string;
}

export interface SliderGroup {
  title: string;
  sliders: SliderMeta[];
}

export const SLIDER_GROUPS: SliderGroup[] = [
  {
    title: 'Land',
    sliders: [
      { key: 'relief', label: 'Relief', help: 'Overall height differences.' },
      { key: 'mountains', label: 'Mountains', help: 'How much of the map is mountain ranges.' },
      { key: 'erosion', label: 'Erosion', help: 'Valleys, gullies and ridges carved by water: the main realism control.' },
      { key: 'roughness', label: 'Roughness', help: 'Small-scale bumpiness of the ground.' },
    ],
  },
  {
    title: 'Water',
    sliders: [
      { key: 'water', label: 'Lakes & wetland', help: 'Lakes, ponds and wetland.' },
      { key: 'rivers', label: 'Rivers', help: 'Number and size of rivers.' },
    ],
  },
  {
    title: 'Life & detail',
    sliders: [
      { key: 'trees', label: 'Trees', help: 'Tree density.' },
      { key: 'vegetation', label: 'Plants', help: 'Grass, flower and plant density.' },
      { key: 'detail', label: 'Ground detail', help: 'Rocks, fallen logs, debris and mixing of ground blocks.' },
    ],
  },
  {
    title: 'Biomes',
    sliders: [
      { key: 'biome_scale', label: 'Region size', help: 'Size of biome regions.' },
      { key: 'biome_blend', label: 'Border blend', help: 'How wide the borders between biomes blend.' },
    ],
  },
];

const KNOWN = new Set(SLIDER_GROUPS.flatMap((g) => g.sliders.map((s) => s.key)));

/** Groups to show for the sliders a settings object actually has. */
export function sliderGroups(keys: string[]): SliderGroup[] {
  const present = new Set(keys);
  const groups = SLIDER_GROUPS.map((g) => ({
    title: g.title,
    sliders: g.sliders.filter((s) => present.has(s.key)),
  })).filter((g) => g.sliders.length > 0);
  const extra = keys.filter((k) => !KNOWN.has(k));
  if (extra.length > 0) {
    groups.push({
      title: 'More',
      sliders: extra.map((key) => ({ key, label: key.replace(/_/g, ' '), help: '' })),
    });
  }
  return groups;
}
