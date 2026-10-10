import { DONATION_FILE, type Donation, parseStart, safeName } from "../src/recordings";

type Mailbox = Pick<Env, "RECORDINGS_TO" | "CONTACT_FROM">;

function megabytes(bytes: number): string {
  return `${(bytes / 1_000_000).toFixed(1)} MB`;
}

export function donationEmail(
  donation: Donation,
  folder: string,
  mailbox: Mailbox,
): EmailMessageBuilder {
  const files = donation.files.map((file) => `- ${safeName(file.name)} (${megabytes(file.size)})`);
  const lines = [
    `Signal: ${donation.signal}`,
    `Email: ${donation.email || "none"}`,
    `Folder: ${folder}`,
    "",
    ...files,
    ...(donation.notes ? ["", donation.notes] : []),
  ];
  return {
    to: mailbox.RECORDINGS_TO,
    from: { email: mailbox.CONTACT_FROM, name: "SDR-- recordings" },
    ...(donation.email ? { replyTo: donation.email } : {}),
    subject: `SDR-- recording: ${donation.signal}`,
    text: lines.join("\n"),
  };
}

async function storedDonation(folder: string, env: Env): Promise<Donation | null> {
  const object = await env.PRIVATE.get(`${folder}${DONATION_FILE}`);
  const parsed = parseStart(object === null ? null : await object.json());
  return parsed.kind === "donation" ? parsed.donation : null;
}

async function complete(folder: string, donation: Donation, env: Env): Promise<boolean> {
  const listed = await env.PRIVATE.list({ prefix: folder });
  const keys = new Set(listed.objects.map((object) => object.key));
  return donation.files.every((file) => keys.has(`${folder}${safeName(file.name)}`));
}

export async function notifyIfDone(key: string, env: Env): Promise<void> {
  const folder = key.slice(0, key.lastIndexOf("/") + 1);
  try {
    const donation = await storedDonation(folder, env);
    if (donation === null) {
      console.error(`recordings: ${folder}${DONATION_FILE} missing or invalid`);
      return;
    }
    if (await complete(folder, donation, env)) {
      await env.EMAIL.send(donationEmail(donation, folder, env));
    }
  } catch (error) {
    console.error(`recordings: notifying about ${folder} failed`, error);
  }
}
