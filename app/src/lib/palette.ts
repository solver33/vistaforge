// Colours the UI uses for biomes, landforms and tree species. Unknown ids (added to the core
// later) get a stable colour from their name.

import { hashText } from './seed';

const BIOMES: Record<string, string> = {
  plains: '#93b85a',
  meadow: '#b9c95e',
  forest: '#3f7a35',
  birch_forest: '#7aa84f',
  taiga: '#36614a',
  pine_forest: '#5d7a3e',
  swamp: '#53693a',
  marsh: '#86a05c',
  highlands: '#a08e5c',
  mountains: '#8b847b',
  alpine_meadow: '#a9c78c',
  snowy_peaks: '#e6edf1',
  beach: '#e0cf98',
  river: '#4d8fc4',
  lake: '#3d79b0',
};

function fromName(id: string): string {
  const h = hashText(id);
  return `hsl(${h % 360} 40% 55%)`;
}

export function biomeColor(id: string): string {
  return BIOMES[id] ?? fromName(id);
}

export interface SpeciesStyle {
  /** Crown shape in the 3D view. */
  crown: 'round' | 'cone' | 'droop' | 'bush' | 'none';
  leaves: string;
  bark: string;
}

const SPECIES: Record<string, SpeciesStyle> = {
  oak: { crown: 'round', leaves: '#3e6a2b', bark: '#5a4632' },
  birch: { crown: 'round', leaves: '#6f9840', bark: '#d9d3c3' },
  spruce: { crown: 'cone', leaves: '#2b4a33', bark: '#4a3828' },
  pine: { crown: 'cone', leaves: '#3c5a34', bark: '#6b4d33' },
  willow: { crown: 'droop', leaves: '#68883f', bark: '#56493a' },
  dead: { crown: 'none', leaves: '#6b5a44', bark: '#6e6152' },
  bush: { crown: 'bush', leaves: '#4b6d2f', bark: '#4b3c2c' },
};

export function speciesStyle(id: string): SpeciesStyle {
  return SPECIES[id] ?? { crown: 'round', leaves: fromName(id), bark: '#5a4632' };
}
