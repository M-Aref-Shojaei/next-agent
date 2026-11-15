import { redirect } from "@sveltejs/kit";

// Settings → Overlay opens on its first section.
export function load() {
  redirect(307, "/settings/overlay/general");
}
