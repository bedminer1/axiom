<script lang="ts">
  import { onMount } from 'svelte';
  import { Activity, Aperture, CircleAlert, ScanLine } from 'lucide-svelte';
  import Hls from 'hls.js';

  type Model = 'unet' | 'sam3';
  type Point = {x:number;y:number};
  type Detection = {label:string;color:string;robot_type:string;confidence:number;x:number;y:number;width:number;height:number;points:Point[]};
  type DetectionFrame = {captured_at_ms:number;inference_ms:number;boxes:Detection[]};
  type Result = {
    image_url:string; mask_url:string; source_name:string; model:string; confidence:number; instances:number;
    metrics:{coverage_percent:number; estimated_length_px:number; demo_estimated_length_mm:number; length_percent_diagonal:number; mean_width_px:number; demo_mean_width_mm:number; mean_width_percent_short_side:number; dominant_orientation_deg:number; orientation_from_horizontal_deg:number; orientation_label:string; foreground_pixels:number; demo_scale_mm_per_px:number};
    grade:{label:string; rationale:string; calibrated:boolean; simulated:boolean};
  };

  const target = 20;
  let results:Result[] = [];
  let loading = false;
  let error = '';
  let failed = 0;
  let showMask = true;
  let model:Model = 'unet';
  let selectedIndex = 0;
  let startedAt = 0;
  let elapsed = 0;
  let timer:ReturnType<typeof setInterval>|undefined;
  let liveVideo:HTMLVideoElement;
  let streamState:'waiting'|'live'|'error'='waiting';
  let detectionBoxes:Detection[]=[];
  let detectionInferenceMs=0;

  onMount(()=>{
    const source='/live/drone.m3u8';
    let hls:Hls|undefined;
    let retry:ReturnType<typeof setTimeout>|undefined;
    let stopped=false;
    const markLive=()=>streamState='live';
    const markWaiting=()=>streamState='waiting';
    const schedule=()=>{if(!stopped){streamState='waiting';if(retry)clearTimeout(retry);retry=setTimeout(connect,1500)}};
    const connect=async()=>{
      try{
        const response=await fetch(source,{cache:'no-store'});
        if(!response.ok)throw new Error('stream offline');
        if(Hls.isSupported()){
          hls?.destroy();
          hls=new Hls({
            lowLatencyMode:true,
            liveSyncDurationCount:1,
            liveMaxLatencyDurationCount:2,
            maxLiveSyncPlaybackRate:1.5,
            maxBufferLength:1,
            backBufferLength:0,
            enableWorker:true
          });
          hls.loadSource(source);
          hls.attachMedia(liveVideo);
          hls.on(Hls.Events.MANIFEST_PARSED,()=>liveVideo.play().catch(()=>{}));
          hls.on(Hls.Events.ERROR,(_event,data)=>{if(data.fatal)schedule()});
        }else if(liveVideo.canPlayType('application/vnd.apple.mpegurl')){
          liveVideo.src=`${source}?t=${Date.now()}`;
          await liveVideo.play().catch(()=>{});
        }else streamState='error';
      }catch{schedule()}
    };
    liveVideo.addEventListener('playing',markLive);
    liveVideo.addEventListener('waiting',markWaiting);
    liveVideo.addEventListener('error',schedule);
    connect();
    return()=>{stopped=true;if(retry)clearTimeout(retry);hls?.destroy();liveVideo.removeEventListener('playing',markLive);liveVideo.removeEventListener('waiting',markWaiting);liveVideo.removeEventListener('error',schedule)};
  });

  onMount(()=>{
    let stopped=false;
    const refresh=async()=>{
      try{
        const response=await fetch(`/live/detections.json?t=${Date.now()}`,{cache:'no-store'});
        if(response.ok){
          const payload:{frames:DetectionFrame[]}=await response.json();
          const targetTime=Date.now()-1500;
          const candidates=payload.frames.filter(frame=>frame.captured_at_ms<=targetTime);
          const frame=candidates.at(-1)??payload.frames.at(-1);
          if(frame&&Date.now()-frame.captured_at_ms<5000){
            detectionBoxes=frame.boxes;
            detectionInferenceMs=frame.inference_ms;
          }else detectionBoxes=[];
        }
      }catch{detectionBoxes=[]}
      if(!stopped)setTimeout(refresh,200);
    };
    refresh();
    return()=>{stopped=true};
  });

  $: selected = results[selectedIndex] ?? null;
  $: totalLength = results.reduce((sum,item)=>sum+item.metrics.demo_estimated_length_mm,0);
  $: averageWidth = results.length ? results.reduce((sum,item)=>sum+item.metrics.demo_mean_width_mm,0)/results.length : 0;
  $: averageCoverage = results.length ? results.reduce((sum,item)=>sum+item.metrics.coverage_percent,0)/results.length : 0;
  $: widest = results.length ? Math.max(...results.map(item=>item.metrics.demo_mean_width_mm)) : 0;
  $: severity = {hairline:results.filter(item=>item.grade.label==='hairline').length,moderate:results.filter(item=>item.grade.label==='moderate').length,wide:results.filter(item=>item.grade.label==='wide').length};

  const fmt=(value:number,digits=1)=>value.toFixed(digits);

  async function requestPhoto():Promise<Result>{
    const response=await fetch('/api/analyze/random',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({model,exclude:results.map(item=>item.source_name)})});
    const body=await response.json();
    if(!response.ok)throw new Error(body.error||'Analysis failed');
    return body;
  }

  async function runJob(){
    loading=true; error=''; failed=0; results=[]; selectedIndex=0; startedAt=Date.now(); elapsed=0;
    timer=setInterval(()=>elapsed=Math.floor((Date.now()-startedAt)/1000),1000);
    let attempts=0;
    while(results.length<target&&attempts<target+10){
      attempts+=1;
      try{results=[...results,await requestPhoto()];selectedIndex=results.length-1}
      catch(caught){failed+=1;error=caught instanceof Error?caught.message:String(caught)}
    }
    if(timer)clearInterval(timer);
    loading=false;
    if(results.length===target)error='';
    else error=`Job completed with ${results.length} of ${target} photos. Last error: ${error}`;
  }
</script>

<svelte:head><title>Axiom — inspection job</title><meta name="description" content="AI concrete crack screening job dashboard"/></svelte:head>

<header><div class="shell nav"><a href="/" class="brand"><span class="mark">AX</span> axiom</a><span class="muted tiny">concrete intelligence / job demo</span></div></header>
<main class="shell">
  <section class="intro">
    <div><div class="eyebrow">Building envelope inspection</div><h1>Twenty photos.<br/><span>One job-level view.</span></h1><p>Analyze a simulated inspection set, review every mask, and aggregate crack geometry into a concise demo report.</p></div>
    <div class="actions"><label><span>model</span><select bind:value={model} disabled={loading}><option value="unet">Crack U-Net · fast</option><option value="sam3">Meta SAM 3 · accurate</option></select></label><button class="button" onclick={runJob} disabled={loading}>{loading?'inspection running…':results.length?'run new job':'run 20-photo job'}</button></div>
  </section>

  <section class="live hairline">
    <video bind:this={liveVideo} autoplay muted playsinline controls aria-label="Live drone camera feed"></video>
    <div class="detections">{#each detectionBoxes as box}<svg class:blue={box.color==='blue'} class:red={box.color==='red'} class="armorShape" viewBox="0 0 1 1" preserveAspectRatio="none" aria-hidden="true"><polygon points={box.points.map(point=>`${point.x},${point.y}`).join(' ')}/></svg><div class:blue={box.color==='blue'} class:red={box.color==='red'} class="detectionLabel" style={`left:${box.x*100}%;top:${box.y*100}%`}><span>{box.label} {Math.round(box.confidence*100)}%</span></div>{/each}</div>
    <div class="liveHud"><span class:online={streamState==='live'}></span><b>{streamState==='live'?'LIVE':streamState==='error'?'UNSUPPORTED':'WAITING FOR DRONE'}</b></div>
    {#if streamState==='live'}<div class="detectHud">ARMOR YOLO · {detectionBoxes.length} plates · {Math.round(detectionInferenceMs)} ms</div>{/if}
    {#if streamState!=='live'}<div class="liveEmpty"><div class="radar"><i></i><i></i><b>AX</b></div><strong>Drone feed offline</strong><small>Publish to the configured RTMP address to begin</small></div>{/if}
    {#if loading}<div class="jobProgress"><span style={`width:${results.length/target*100}%`}></span><small>Analyzing {results.length}/{target} · {elapsed}s {failed?`· ${failed} retries`:''}</small></div>{/if}
  </section>

  {#if error&&!loading}<div class="error hairline"><CircleAlert size={18}/><div><strong>Job completed with warnings</strong><p>{error}</p></div></div>{/if}

  {#if results.length}
    <section class="summary hairline">
      <div class="summaryHead"><div><div class="eyebrow">job summary</div><h2>Building envelope · inspection 001</h2></div><span class="demoTag">simulated scale</span></div>
      <div class="summaryGrid"><article><span>photos analyzed</span><b>{results.length}/{target}</b></article><article><span>total crack length</span><b>{fmt(totalLength,0)} <small>mm*</small></b></article><article><span>average width</span><b>{fmt(averageWidth,2)} <small>mm*</small></b></article><article><span>average coverage</span><b>{fmt(averageCoverage,2)}%</b></article><article><span>widest mean</span><b>{fmt(widest,2)} <small>mm*</small></b></article></div>
      <div class="bands"><span>hairline <b>{severity.hairline}</b></span><span>moderate <b>{severity.moderate}</b></span><span>wide <b>{severity.wide}</b></span></div>
      <p class="disclaimer">* Demo values assume 0.12 mm/px. Measurements and severity bands are fabricated for presentation, not engineering use.</p>
    </section>

    <section class="strip hairline"><div class="stripHead"><span class="eyebrow">inspection photos</span><span>{results.length} records</span></div><div class="thumbs">{#each results as item,index}<button class:active={index===selectedIndex} onclick={()=>selectedIndex=index}><img src={item.image_url} alt="Inspection thumbnail"/><span>{String(index+1).padStart(2,'0')}</span><i class={item.grade.label}></i></button>{/each}</div></section>
  {/if}

  {#if selected}
    <section class="workspace">
      <div class="viewer hairline"><div class="viewerbar"><div><span class="eyebrow">photo {selectedIndex+1} / {results.length}</span><strong>{selected.source_name}</strong></div><label><input type="checkbox" bind:checked={showMask}/> mask overlay</label></div><div class="canvas"><img src={selected.image_url} alt="Concrete crack sample"/>{#if showMask}<img class="mask" src={selected.mask_url} alt="Predicted crack mask"/>{/if}<div class="reticle"></div></div></div>
      <aside><div class="grade hairline"><div class="eyebrow">demo severity</div><div class="gradeword">{selected.grade.label}</div><p>{selected.grade.rationale}</p></div><div class="metrics"><article class="hairline"><Aperture size={16}/><span class="eyebrow">coverage</span><div class="value">{fmt(selected.metrics.coverage_percent,2)}%</div><small>image area</small></article><article class="hairline"><Activity size={16}/><span class="eyebrow">length</span><div class="value">{fmt(selected.metrics.demo_estimated_length_mm)} mm*</div><small>{fmt(selected.metrics.estimated_length_px)} px proxy</small></article><article class="hairline"><div class="orientation"><span>H</span><i class="angle" style={`transform:rotate(${selected.metrics.dominant_orientation_deg}deg)`}></i><span>V</span></div><span class="eyebrow">direction</span><div class="value direction">{selected.metrics.orientation_label}</div><small>{fmt(selected.metrics.orientation_from_horizontal_deg)}° from horizontal</small></article><article class="hairline"><span class="widthIcon"></span><span class="eyebrow">mean width</span><div class="value">{fmt(selected.metrics.demo_mean_width_mm,2)} mm*</div><small>{fmt(selected.metrics.mean_width_px,2)} px</small></article></div><div class="meta hairline"><div><span>model</span><b>{selected.model}</b></div><div><span>worker score</span><b>{fmt(selected.confidence*100)}%</b></div><div><span>components</span><b>{selected.instances}</b></div></div></aside>
    </section>
  {:else if !loading}
    <div class="empty hairline"><ScanLine size={28}/><p>No inspection job analyzed yet.</p></div>
  {/if}
  <footer><span>Screening demo only — not a structural engineering assessment.</span><span>* fabricated scale and severity</span></footer>
</main>

<style>
header{border-bottom:1px solid #171717}.nav{height:58px;display:flex;align-items:center;justify-content:space-between}.brand{color:#eee;text-decoration:none;font-size:13px}.mark{display:inline-grid;place-items:center;width:27px;height:27px;border:1px solid #333;border-radius:5px;margin-right:9px}.tiny{font-size:10px}.intro{padding:68px 0 38px;display:flex;align-items:flex-end;justify-content:space-between;gap:40px}.intro h1{font-size:clamp(38px,6vw,72px);line-height:1.02;letter-spacing:-.065em;margin:15px 0 20px;font-weight:500}.intro h1 span{color:#777}.intro p{color:#999;max-width:680px;font-size:13px;line-height:1.75;margin:0}.actions{display:grid;gap:8px;min-width:230px}.actions label{display:flex;align-items:center;justify-content:space-between;gap:12px;border:1px solid #292929;border-radius:7px;padding:8px 10px}.actions label span{font-size:9px;text-transform:uppercase;color:#777}.actions select{border:0;background:#080808;color:#ddd;font-size:10px;outline:0}.empty{height:320px;display:grid;place-content:center;text-align:center;color:#666}.error{padding:18px;display:flex;gap:14px;border-color:#543232;color:#d2a3a3;margin-bottom:14px}.error p{font-size:11px;margin:7px 0 0}.loading{height:420px;display:grid;place-content:center;text-align:center;position:relative;overflow:hidden}.scanner{position:absolute;inset-inline:0;height:90px;background:linear-gradient(transparent,#6ee7b711,transparent);animation:scan 2.2s linear infinite}.radar{width:92px;height:92px;margin:0 auto 24px;display:grid;place-items:center;position:relative}.radar i{position:absolute;inset:0;border:1px solid #6ee7b744;border-radius:50%;animation:pulse 1.8s ease-in-out infinite}.radar i:nth-child(2){inset:14px;animation-delay:.25s}.radar b{display:grid;place-items:center;width:38px;height:38px;border-radius:50%;background:#b7f7d5;color:#07110c;font-size:11px}.progressTitle{font-size:26px;margin:10px 0 6px}.loading p,.loading small{font-size:9px;color:#777}.track{width:300px;height:3px;background:#1d1d1d;margin:20px auto 10px;border-radius:9px;overflow:hidden}.track span{display:block;height:100%;background:#9ff0c5;transition:width .4s}.summary{padding:20px;margin-bottom:14px}.summaryHead{display:flex;justify-content:space-between;align-items:start}.summary h2{font-size:18px;font-weight:400;margin:8px 0 18px}.demoTag{font-size:8px;text-transform:uppercase;color:#e9c46a;border:1px solid #e9c46a44;border-radius:99px;padding:5px 8px}.summaryGrid{display:grid;grid-template-columns:repeat(5,1fr);gap:1px;background:#202020;border:1px solid #202020;border-radius:8px;overflow:hidden}.summaryGrid article{background:#050505;padding:14px}.summaryGrid span{font-size:8px;text-transform:uppercase;color:#777}.summaryGrid b{font-size:21px;font-weight:400;display:block;margin-top:10px}.summaryGrid small{font-size:9px;color:#777}.bands{display:flex;gap:8px;margin-top:12px}.bands span{font-size:9px;color:#888;border:1px solid #252525;border-radius:99px;padding:6px 10px}.bands b{color:#ddd}.disclaimer{font-size:8px;color:#595959;margin:13px 0 0}.strip{margin-bottom:14px;padding:12px}.stripHead{display:flex;justify-content:space-between;margin:0 3px 10px;font-size:9px;color:#666}.thumbs{display:grid;grid-template-columns:repeat(10,1fr);gap:6px}.thumbs button{position:relative;aspect-ratio:1;border:1px solid transparent;border-radius:5px;overflow:hidden;padding:0;background:#111;cursor:pointer}.thumbs button.active{border-color:#eee}.thumbs img{width:100%;height:100%;object-fit:cover}.thumbs span{position:absolute;left:4px;bottom:3px;font-size:7px;background:#000b;padding:2px}.thumbs i{position:absolute;right:4px;top:4px;width:5px;height:5px;border-radius:50%;background:#7a7a7a}.thumbs i.moderate{background:#e9c46a}.thumbs i.wide{background:#e76f51}.workspace{display:grid;grid-template-columns:minmax(0,1.7fr) minmax(290px,.7fr);gap:14px}.viewer{overflow:hidden}.viewerbar{height:64px;border-bottom:1px solid #1f1f1f;padding:12px 15px;display:flex;justify-content:space-between;align-items:center}.viewerbar strong{display:block;font-size:11px;margin-top:6px;max-width:420px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.viewerbar label{font-size:10px;color:#999;display:flex;align-items:center;gap:8px}.canvas{aspect-ratio:1;position:relative;background:#0e0e0e;overflow:hidden}.canvas img{position:absolute;width:100%;height:100%;object-fit:contain}.canvas .mask{mix-blend-mode:screen;opacity:.72;filter:sepia(1) saturate(15) hue-rotate(315deg)}.reticle{position:absolute;inset:10px;border:1px solid #ffffff0c;pointer-events:none}aside{display:flex;flex-direction:column;gap:12px}.grade{padding:18px}.gradeword{font-size:32px;letter-spacing:-.05em;text-transform:uppercase;margin:12px 0}.grade p{color:#8c8c8c;font-size:10px;line-height:1.6;margin:0}.metrics{display:grid;grid-template-columns:1fr 1fr;gap:8px}.metrics article{padding:14px;min-height:138px;display:flex;flex-direction:column;justify-content:space-between}.metrics small{color:#666;font-size:8px;line-height:1.4}.direction{font-size:13px;line-height:1.1}.orientation{display:flex;align-items:center;gap:5px;color:#555;font-size:7px}.angle{display:inline-block;width:24px;border-top:1px solid #aaa;transform-origin:center}.widthIcon{width:18px;height:8px;border-top:1px solid #aaa;border-bottom:1px solid #aaa}.meta{padding:5px 14px}.meta div{display:flex;justify-content:space-between;gap:15px;padding:11px 0;border-bottom:1px solid #171717;font-size:10px}.meta div:last-child{border:0}.meta span{color:#777}.meta b{font-weight:400;text-align:right}footer{display:flex;justify-content:space-between;color:#555;font-size:9px;padding:34px 0 22px}@keyframes scan{from{transform:translateY(-100px)}to{transform:translateY(430px)}}@keyframes pulse{0%,100%{transform:scale(.8);opacity:.3}50%{transform:scale(1);opacity:1}}@media(max-width:800px){.intro{padding-top:44px;align-items:flex-start;flex-direction:column}.workspace{grid-template-columns:1fr}.summaryGrid{grid-template-columns:1fr 1fr}.thumbs{grid-template-columns:repeat(5,1fr)}.tiny,footer span:last-child{display:none}.metrics{grid-template-columns:1fr 1fr}}
.live{position:relative;aspect-ratio:16/9;overflow:hidden;margin-bottom:14px;background:#030504}.live video{width:100%;height:100%;display:block;object-fit:contain;background:#000}.liveHud{position:absolute;top:14px;left:14px;display:flex;align-items:center;gap:7px;padding:6px 9px;border-radius:5px;background:#050505cc;font-size:9px;letter-spacing:.08em}.liveHud span{width:7px;height:7px;border-radius:50%;background:#777}.liveHud span.online{background:#ef4444;box-shadow:0 0 10px #ef4444}.detectHud{position:absolute;top:14px;right:14px;padding:6px 9px;border-radius:5px;background:#050505cc;color:#b7f7d5;font-size:9px;letter-spacing:.04em}.detections{position:absolute;inset:0;pointer-events:none}.armorShape{position:absolute;inset:0;width:100%;height:100%;overflow:visible}.armorShape polygon{fill:#b7f7d519;stroke:#b7f7d5;stroke-width:2px;vector-effect:non-scaling-stroke;filter:drop-shadow(0 0 4px #000)}.armorShape.blue polygon{fill:#60a5fa26;stroke:#60a5fa}.armorShape.red polygon{fill:#f8717126;stroke:#f87171}.detectionLabel{position:absolute;transform:translateY(-22px)}.detectionLabel span{display:block;background:#b7f7d5;color:#041109;padding:4px 6px;font-size:9px;font-weight:600;white-space:nowrap}.detectionLabel.blue span{background:#60a5fa;color:#061326}.detectionLabel.red span{background:#f87171;color:#260606}.liveEmpty{position:absolute;inset:0;display:grid;place-content:center;text-align:center;pointer-events:none}.liveEmpty strong{font-size:13px;font-weight:500}.liveEmpty small{margin-top:8px;color:#666;font-size:9px}.jobProgress{position:absolute;inset:auto 0 0;height:26px;background:#050505dd}.jobProgress>span{display:block;height:2px;background:#9ff0c5;transition:width .4s}.jobProgress small{display:block;padding:7px 10px;color:#999;font-size:8px}@media(max-width:800px){.live{aspect-ratio:16/9}}
</style>
