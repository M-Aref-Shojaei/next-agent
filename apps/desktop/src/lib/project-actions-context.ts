import { createContext } from "svelte";

export const [getShowProjectActions, setShowProjectActions] = createContext<() => void>();
