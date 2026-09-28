/**
 * Minimal jeden extension fixture. Seeded into a sandbox `.jeden/extensions`
 * so the extensions view has something real to discover — a functional
 * contract needs an artifact, not an empty state.
 */
export const tools = [
  {
    name: 'probe_tool',
    description: 'Fixture tool used by the probierz extension-discovery contract.',
    parameters: { type: 'object', properties: {} },
    run: async () => 'probe',
  },
];

export default { name: 'probe-ext', tools };
