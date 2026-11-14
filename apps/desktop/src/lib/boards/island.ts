// Lifecycle for a framework island: a component of another UI library (React,
// for Excalidraw) mounted into a DOM node owned by Svelte. This file knows
// nothing about React so the lifecycle can be unit-tested; react-mount.ts
// supplies the real renderer.

export interface IslandRenderer<P> {
  render(props: P): void;
  unmount(): void;
}

export interface Island<P> {
  /** Re-render with changed props. No-op after destroy. */
  update(props: Partial<P>): void;
  /** Unmount. Safe to call more than once. */
  destroy(): void;
  readonly mounted: boolean;
  /** Settles after the first render (or after an early destroy). Rejects if `create` fails. */
  readonly ready: Promise<void>;
}

/**
 * Start an island and return its handle at once. `create` may be async (a
 * dynamic import): updates and a `destroy` that arrive while it loads are
 * honoured, and `ready` settles when the first render is done.
 */
export function mountIsland<P>(
  create: () => Promise<IslandRenderer<P>> | IslandRenderer<P>,
  initial: P,
): Island<P> {
  let props = initial;
  let renderer: IslandRenderer<P> | null = null;
  let destroyed = false;
  const island: Island<P> = {
    update(next) {
      if (destroyed) return;
      props = { ...props, ...next };
      renderer?.render(props);
    },
    destroy() {
      if (destroyed) return;
      destroyed = true;
      renderer?.unmount();
      renderer = null;
    },
    get mounted() {
      return renderer !== null && !destroyed;
    },
    ready: Promise.resolve(create()).then((created) => {
      if (destroyed) {
        created.unmount();
        return;
      }
      renderer = created;
      renderer.render(props);
    }),
  };
  return island;
}
