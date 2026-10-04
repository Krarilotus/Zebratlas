/** Curated official links; not graph assertions, partnerships, or data-access grants. */
export interface NetworkDirectoryEntry {
  id: string;
  name: string;
  group: 'support' | 'research' | 'data';
  scope: { en: string; de: string };
  region: 'global' | 'regional';
  purpose: { en: string; de: string };
  limitation: { en: string; de: string };
  links: { label: { en: string; de: string }; url: string; kind: 'information' | 'contact' | 'data' }[];
  sources: { url: string; checkedAt: string }[];
}

const checkedAt = '2026-10-04';
const labels: Record<string, { en: string; de: string }[]> = {
  rdi: [{ en: 'Contact RDI', de: 'RDI kontaktieren' }, { en: 'Find member groups', de: 'Mitgliedsgruppen finden' }],
  irdirc: [{ en: 'Contact secretariat', de: 'Sekretariat kontaktieren' }, { en: 'Research coordination', de: 'Forschungskoordination' }],
  'global-genes': [{ en: 'Community and programme contacts', de: 'Gemeinschafts- und Programmkontakte' }, { en: 'RARE-X research access', de: 'RARE-X-Forschungszugang' }],
  nord: [{ en: 'Contact NORD', de: 'NORD kontaktieren' }],
  orphanet: [{ en: 'Contact national teams', de: 'Nationale Teams kontaktieren' }],
  matchmaker: [{ en: 'Participating services', de: 'Teilnehmende Dienste' }],
  elixir: [{ en: 'Community contact', de: 'Community-Kontakt' }, { en: 'Get involved', de: 'Mitwirken' }],
};
const entry = (id: string, name: string, group: NetworkDirectoryEntry['group'], region: NetworkDirectoryEntry['region'], scope: NetworkDirectoryEntry['scope'], purpose: NetworkDirectoryEntry['purpose'], limitation: NetworkDirectoryEntry['limitation'], url: string, extra?: string): NetworkDirectoryEntry => ({
  id, name, group, region, scope, purpose, limitation,
  links: [{ label: labels[id][0], url, kind: 'contact' }, ...(extra ? [{ label: labels[id][1], url: extra, kind: 'information' as const }] : [])],
  sources: [url, ...(extra ? [extra] : [])].map(url => ({ url, checkedAt })),
});

export const networkDirectory: readonly NetworkDirectoryEntry[] = [
  {
    id: 'eurordis', name: 'EURORDIS – Rare Diseases Europe', group: 'support', region: 'regional',
    scope: { en: 'Europe; international member organisations', de: 'Europa; internationale Mitgliedsorganisationen' },
    purpose: { en: 'Patient advocacy, research, policy and community resources.', de: 'Patientenvertretung, Forschung, Politik und Gemeinschaftsressourcen.' },
    limitation: { en: 'Programme scope and participation vary; no individual medical advice.', de: 'Programmumfang und Teilnahme variieren; keine individuelle medizinische Beratung.' },
    links: [
      { label: { en: 'Patient alliance', de: 'Patientenallianz' }, url: 'https://eurordis.org', kind: 'information' },
      { label: { en: 'Contact EURORDIS', de: 'EURORDIS kontaktieren' }, url: 'mailto:eurordis@eurordis.org', kind: 'contact' },
    ],
    sources: [{ url: 'https://download2.eurordis.org/EURORDIS_flyer_digital.pdf', checkedAt }],
  },
  entry('rdi', 'Rare Diseases International', 'support', 'global', { en: 'Global patient alliance', de: 'Weltweite Patientenallianz' }, { en: 'Find national and international patient organisations.', de: 'Nationale und internationale Patientenorganisationen finden.' }, { en: 'Member coverage varies by country and condition.', de: 'Die Mitgliedsabdeckung variiert nach Land und Erkrankung.' }, 'https://www.rarediseasesinternational.org/contact/', 'https://www.rarediseasesinternational.org/members-list/'),
  entry('irdirc', 'IRDiRC', 'research', 'global', { en: 'International research coordination', de: 'Internationale Forschungskoordination' }, { en: 'Connect research priorities, resources and collaboration through its Scientific Secretariat.', de: 'Forschungsprioritäten, Ressourcen und Zusammenarbeit über das wissenschaftliche Sekretariat verbinden.' }, { en: 'Research consortium, not a clinical referral or patient-data intake service.', de: 'Forschungskonsortium, kein klinischer Vermittlungsdienst oder Eingang für Patientendaten.' }, 'https://irdirc.org/contact-us/', 'https://irdirc.org/why-we-exist/'),
  entry('global-genes', 'Global Genes / RARE-X', 'support', 'global', { en: 'US-based; international community links', de: 'Sitz in den USA; internationale Gemeinschaftslinks' }, { en: 'Community support, advocacy connections and governed patient-led data programmes.', de: 'Gemeinschaftshilfe, Vernetzung und geregelte patientengeführte Datenprogramme.' }, { en: 'RARE-X participation and research access have separate consent and access requirements.', de: 'RARE-X-Teilnahme und Forschungszugang haben eigene Einwilligungs- und Zugangsbedingungen.' }, 'https://globalgenes.org/contact-us/', 'https://rare-x.org/researchers-access/'),
  entry('nord', 'NORD', 'support', 'regional', { en: 'United States', de: 'USA' }, { en: 'Patient-organisation discovery, research programmes and IAMRARE registry enquiries.', de: 'Patientenorganisationen finden sowie Forschungsprogramme und IAMRARE-Register anfragen.' }, { en: 'US programmes; eligibility and access vary.', de: 'US-Programme; Teilnahme und Zugang variieren.' }, 'https://rarediseases.org/contact/'),
  entry('orphanet', 'Orphanet', 'research', 'regional', { en: 'International network; national teams', de: 'Internationales Netzwerk; nationale Teams' }, { en: 'Find expert resources and contact national teams about professional or institutional information.', de: 'Expertenressourcen finden und nationale Teams zu Fach- oder Institutionsinformationen kontaktieren.' }, { en: 'National professional enquiries; not individual medical advice.', de: 'Nationale fachliche Anfragen; keine individuelle medizinische Beratung.' }, 'https://www.orpha.net/en/institutions/get-in-touch'),
  entry('matchmaker', 'Matchmaker Exchange', 'data', 'global', { en: 'International federated matching', de: 'Internationaler föderierter Abgleich' }, { en: 'Find participating services connecting rare-disease investigators and cases.', de: 'Teilnehmende Dienste zur Vernetzung von Forschenden und seltenen Erkrankungsfällen finden.' }, { en: 'Each node has its own user, consent and submission rules; no universal patient access.', de: 'Jeder Knoten hat eigene Nutzer-, Einwilligungs- und Einreichungsregeln; kein universeller Patientenzugang.' }, 'https://www.matchmakerexchange.org/participants.html'),
  entry('elixir', 'ELIXIR Rare Diseases Community', 'data', 'regional', { en: 'Europe; international collaboration', de: 'Europa; internationale Zusammenarbeit' }, { en: 'Contact the community liaison about FAIR data resources, tools and research infrastructure.', de: 'Die Community-Ansprechperson zu FAIR-Datenressourcen, Werkzeugen und Forschungsinfrastruktur kontaktieren.' }, { en: 'Tools and collaboration routes are not permission to access controlled patient data.', de: 'Werkzeuge und Kooperationswege erlauben keinen Zugriff auf geschützte Patientendaten.' }, 'https://elixir-europe.org/communities/rare-diseases', 'https://elixir-europe.org/about-us/get-involved'),
];
