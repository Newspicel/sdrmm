import { CONTACT_PATH } from "../src/contact";
import { RECORDINGS_PATH } from "../src/recordings";
import { missing } from "./assets";
import { contact } from "./contact";
import { recordings } from "./recordings";

export default {
  fetch(request, env) {
    const { pathname } = new URL(request.url);
    if (pathname === CONTACT_PATH) {
      return contact(request, env);
    }
    if (pathname.startsWith(`${RECORDINGS_PATH}/`)) {
      return recordings(request, env);
    }
    return missing(request, env);
  },
} satisfies ExportedHandler<Env>;
