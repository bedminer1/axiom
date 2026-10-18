import { photoStore } from '$lib/photo-store';
import { Document, HeadingLevel, ImageRun, Packer, Paragraph, Table, TableCell, TableRow, TextRun, WidthType } from 'docx';

type Project = {
  id: string; name: string; address: string; owner?: string; noticeReference?: string;
  elevations: string[]; systems: string[]; usesDrone?: boolean;
  visualDate?: string; visualDuration?: string; visualMethod?: string; visualLimitations?: string;
  closeDate?: string; closeDuration?: string; closeMethod?: string; closeBasis?: string;
  closeCoverage?: Record<string, number>; previousCondition?: string; inspector?: string;
  visualDateAuto?: boolean; closeDateAuto?: boolean;
};
type Slot = {
  name: string; analysis: string; reviewed: boolean; level?: string; zone?: string;
  defectType?: string; classification?: string; extent?: string; structuralConcern?: string;
  verification?: string; possibleCause?: string; recommendation?: string; timeframe?: string;
  immediateAction?: string; supervision?: string;
  photoDate?: string; storeysVisible?: number;
  side?: string; mapX?: number; mapY?: number; mapSource?: string;
};
type Entry = { id: string; number: string; section: string; side: string; slot: Slot };

const value = (text?: string) => text?.trim() || 'Not recorded';
const heading = (text: string, level: (typeof HeadingLevel)[keyof typeof HeadingLevel] = HeadingLevel.HEADING_1) => new Paragraph({
  children: [new TextRun({ text, color: '111111', bold: true })],
  heading: level, keepNext: true, pageBreakBefore: /^Section [345]/.test(text),
  spacing: { before: 260, after: 120 }
});
const cell = (text: string) => new TableCell({ children: [new Paragraph({ text, spacing: { after: 0 } })] });
const table = (items: [string, string][]) => new Table({
  width: { size: 100, type: WidthType.PERCENTAGE },
  columnWidths: [3000, 7200],
  rows: items.map(([label, detail]) => new TableRow({ children: [cell(label), cell(detail)] }))
});

async function photoParagraph(project: Project, entry: Entry) {
  const photo = await photoStore(`${project.id}:${entry.id}`);
  if (!photo) throw new Error(`Photo missing for ${entry.number}. Add it again before downloading.`);
  const bitmap = await createImageBitmap(photo);
  const scale = Math.min(510 / bitmap.width, 340 / bitmap.height, 1);
  const transformation = { width: Math.round(bitmap.width * scale), height: Math.round(bitmap.height * scale) };
  let data: Uint8Array;
  let type: 'jpg' | 'png';
  if (photo.type === 'image/jpeg' || photo.type === 'image/png') {
    data = new Uint8Array(await photo.arrayBuffer());
    type = photo.type === 'image/jpeg' ? 'jpg' : 'png';
  } else {
    const canvas = document.createElement('canvas');
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    canvas.getContext('2d')!.drawImage(bitmap, 0, 0);
    const png = await new Promise<Blob>((resolve, reject) => canvas.toBlob((result) => result ? resolve(result) : reject(new Error('Could not prepare photo.')), 'image/png'));
    data = new Uint8Array(await png.arrayBuffer());
    type = 'png';
  }
  bitmap.close();
  return new Paragraph({ children: [new ImageRun({ data, type, transformation, altText: { name: entry.number, description: entry.slot.name } })], keepNext: true });
}

async function planParagraph(plan: Blob | undefined, entries: Entry[]) {
  const canvas = document.createElement('canvas');
  canvas.width = 600; canvas.height = 450;
  const context = canvas.getContext('2d')!;
  context.fillStyle = '#f6f4ef'; context.fillRect(0, 0, 600, 450);
  if (plan) {
    const bitmap = await createImageBitmap(plan);
    context.drawImage(bitmap, 0, 0, 600, 450);
    bitmap.close();
  } else {
    context.strokeStyle = '#555555'; context.lineWidth = 3; context.strokeRect(90, 72, 420, 306);
    context.fillStyle = '#333333'; context.font = 'bold 24px Arial'; context.textAlign = 'center';
    context.fillText('N', 300, 42); context.fillText('S', 300, 421);
    context.fillText('W', 38, 232); context.fillText('E', 562, 232);
  }
  context.textAlign = 'center'; context.textBaseline = 'middle'; context.font = 'bold 16px Arial';
  for (const entry of entries.filter((item) => item.section === 'close')) {
    const x = (entry.slot.mapX ?? 50) * 6;
    const y = (entry.slot.mapY ?? 50) * 4.5;
    context.fillStyle = '#e87528'; context.beginPath(); context.arc(x, y, 16, 0, Math.PI * 2); context.fill();
    context.strokeStyle = '#ffffff'; context.lineWidth = 3; context.stroke();
    context.fillStyle = '#111111'; context.fillText(entry.number.slice(1), x, y + 1);
  }
  const blob = await new Promise<Blob>((resolve, reject) => canvas.toBlob((value) => value ? resolve(value) : reject(new Error('Could not render plan.')), 'image/png'));
  return new Paragraph({ children: [new ImageRun({ data: new Uint8Array(await blob.arrayBuffer()), type: 'png', transformation: { width: 510, height: 383 }, altText: { name: 'Inspection location plan', description: plan ? 'Uploaded floor plan with close-range photo markers' : 'Schematic orientation with close-range photo markers' } })] });
}

export async function downloadReport(project: Project, slots: Record<string, Slot>) {
  const entries: Entry[] = [];
  project.elevations.forEach((side, index) => {
    const id = `visual:${side}`;
    const slot = slots[id];
    if (!slot?.reviewed || !slot.analysis.trim()) throw new Error(`${side} visual inspection still needs review.`);
    entries.push({ id, number: `V${String(index + 1).padStart(2, '0')}`, section: 'visual', side, slot });
  });
  const closePhotos = Object.entries(slots).filter(([id]) => id.startsWith('close:'));
  if (!closePhotos.length) throw new Error('Add at least one close-range photo.');
  closePhotos.forEach(([id, slot], index) => {
    if (!slot.reviewed || !slot.analysis.trim()) throw new Error(`Close-range photo ${index + 1} still needs review.`);
    entries.push({ id, number: `C${String(index + 1).padStart(2, '0')}`, section: 'close', side: slot.side || 'Side not recorded', slot });
  });
  const maxStoreys = Math.max(0, ...entries.map((entry) => entry.slot.storeysVisible ?? 0));
  const plan = await photoStore(`${project.id}:floor-plan`);
  const planBitmap = plan ? await createImageBitmap(plan) : null;
  const markedPlan = await planParagraph(plan, entries);
  const planPosition = (entry: Entry) => entry.section === 'close' && entry.slot.mapX != null && entry.slot.mapY != null
    ? planBitmap
      ? ` · plan x ${Math.round(entry.slot.mapX * planBitmap.width / 100)} px, y ${Math.round(entry.slot.mapY * planBitmap.height / 100)} px (${entry.slot.mapSource || 'suggested'})`
      : ` · schematic ${entry.slot.mapX}%, ${entry.slot.mapY}% (${entry.slot.mapSource || 'suggested'})`
    : '';
  const blocks: (Paragraph | Table)[] = [
    new Paragraph({ children: [new TextRun({ text: 'Periodic Façade Inspection Report', color: '111111', bold: true })], heading: HeadingLevel.TITLE }),
    new Paragraph({ text: `Draft · ${project.name} · ${project.address}` }),
    new Paragraph({ text: `Notice reference: ${value(project.noticeReference)}     Owner / MCST: ${value(project.owner)}` }),
    heading('Section 2 – 100% Visual Inspection'),
    heading('2.1 Diary and Scope of Visual Inspection', HeadingLevel.HEADING_2),
    table([
      ['Date and duration', `${value(project.visualDate)}${project.visualDateAuto ? ' (suggested)' : ''} · ${value(project.visualDuration)}`],
      ['Location', `${project.name}, ${project.address}`],
      ...(maxStoreys ? [['Storeys visible in photos', `At least ${maxStoreys} (AI estimate, not confirmed building height)`] as [string, string]] : []),
      ['Method and process', value(project.visualMethod)],
      ['Drone used', project.usesDrone ? 'Yes' : 'No'],
      ['Inspection personnel', value(project.inspector)],
      ['Areas not covered or deviations', value(project.visualLimitations)]
    ]),
    heading('2.2 Key Findings of the Visual Inspection', HeadingLevel.HEADING_2)
  ];
  for (const section of ['visual', 'close']) {
    if (section === 'close') blocks.push(
      heading('Section 3 – Minimum 10% Close-range Inspection'),
      heading('3.1 Diary and Scope of Close-range Inspection', HeadingLevel.HEADING_2),
      table([
        ['Basis for selected areas', value(project.closeBasis)],
        ['Date and duration', `${value(project.closeDate)}${project.closeDateAuto ? ' (suggested)' : ''} · ${value(project.closeDuration)}`],
        ['Method, tools and equipment', value(project.closeMethod)],
        ['Inspection personnel', value(project.inspector)],
        ...project.elevations.map((elevation): [string, string] => [`${elevation} inspected area`, project.closeCoverage?.[elevation] ? `${project.closeCoverage?.[elevation]}% (reviewer entered)` : 'Not recorded'])
      ]),
      heading('Close-range photo locations', HeadingLevel.HEADING_2),
      markedPlan,
      new Paragraph({ text: plan ? 'Markers show suggested or reviewer-placed positions on the uploaded floor plan.' : 'Schematic orientation from elevation labels; positions are approximate and not an as-built plan.' }),
      heading('3.2 Key Findings of the Close-range Inspection', HeadingLevel.HEADING_2)
    );
    for (const entry of entries.filter((item) => item.section === section)) blocks.push(
      heading(`Photo ${entry.number} – ${entry.side}${section === 'visual' ? ' elevation' : ''}`, HeadingLevel.HEADING_3),
      await photoParagraph(project, entry),
      table([
        ['Location', `${entry.side} · Level ${value(entry.slot.level)} · ${value(entry.slot.zone)}${planPosition(entry)}`],
        ['Photo date', value(entry.slot.photoDate)],
        ['Storeys visible (AI estimate)', entry.slot.storeysVisible != null ? String(entry.slot.storeysVisible) : 'Not recorded'],
        ['Observation', entry.slot.analysis.trim()],
        ['Visible defect', value(entry.slot.defectType)],
        ['Nature of defect', value(entry.slot.classification)],
        ['Façade integrity concern', value(entry.slot.structuralConcern)],
        ['Checks needed', value(entry.slot.verification)]
      ])
    );
  }
  blocks.push(
    heading('Section 4 – Assessment of the Defects'),
    heading('4(a) Comparison with the previous inspection', HeadingLevel.HEADING_2),
    new Paragraph({ text: `Previous findings provided: ${value(project.previousCondition)}` }),
    new Paragraph({ text: 'Current reviewed findings are listed below. The Competent Person should confirm any change from the previous cycle.' }),
    table(entries.map((entry) => [
      `${entry.number} · ${entry.side} · Level ${value(entry.slot.level)}`,
      `${value(entry.slot.defectType)} · ${value(entry.slot.classification)} · ${entry.slot.classification === 'Safe' ? 'No defect recorded' : value(entry.slot.extent)} · ${value(entry.slot.structuralConcern)}`
    ])),
    heading('4(b) Localised or systemic assessment', HeadingLevel.HEADING_2),
    new Paragraph({ text: 'The extent above is the reviewer’s assessment for each photographed condition. A photograph alone cannot establish concealed attachment or structural soundness; complete the listed physical checks before reaching that conclusion.' }),
    heading('Section 5 – Recommendations, Remedial and Follow-up Actions'),
    new Paragraph({ text: 'The Competent Person should confirm that each proposal addresses the affected and similar façade areas, with appropriate method, materials, precautions and supervision.' })
  );
  for (const classification of ['Unsafe', 'Require Repair']) {
    const findings = entries.filter((entry) => entry.slot.classification === classification);
    blocks.push(heading(`${classification} defects`, HeadingLevel.HEADING_2));
    if (!findings.length) blocks.push(new Paragraph({ text: `No reviewed photo was classified ${classification}.` }));
    for (const entry of findings) blocks.push(
      heading(`Photo ${entry.number} – ${entry.side}`, HeadingLevel.HEADING_3),
      table([
        ['Location', `${entry.side} · Level ${value(entry.slot.level)} · ${value(entry.slot.zone)}${planPosition(entry)}`],
        ['Observation', entry.slot.analysis.trim()],
        ['Possible cause', value(entry.slot.possibleCause)],
        ['Recommendation', value(entry.slot.recommendation)],
        ['Timeframe', value(entry.slot.timeframe)],
        ['CP supervision', value(entry.slot.supervision)],
        ...(classification === 'Unsafe' ? [['Immediate action proposed', value(entry.slot.immediateAction)] as [string, string]] : [])
      ])
    );
  }
  const file = new Document({
    styles: { default: { document: { run: { font: 'Arial', size: 20 }, paragraph: { spacing: { after: 100 } } } } },
    sections: [{ properties: { page: { size: { width: 11906, height: 16838 }, margin: { top: 850, right: 850, bottom: 850, left: 850 } } }, children: blocks }]
  });
  planBitmap?.close();
  const url = URL.createObjectURL(await Packer.toBlob(file));
  const link = document.createElement('a');
  link.href = url;
  link.download = `${project.name.trim().replace(/[^a-z0-9-]+/gi, '-').replace(/^-|-$/g, '') || 'hound'}-pfi-draft.docx`;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}
