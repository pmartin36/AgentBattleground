// combat — "motion + effects" attack as an ANIMATION STATE MACHINE.
// Attacker plays an idle clip, SWAPS to a lunge/attack clip during the attack (real generated body
// motion), then back to idle. An authored braille slash effect + target flash/recoil layer on top.
// The creature motion is generic (weaponless-capable); the effect is the pluggable attack element.
//
// Usage: combat <atk_idle_dir> <atk_attack_dir> <target_idle_dir> [--fps N]

use crossterm::{event::{self, Event, KeyCode}, execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen}};
use image::{imageops, DynamicImage, GenericImageView, RgbaImage};
use ratatui::{backend::CrosstermBackend, style::{Color, Style}, text::{Line, Span}, widgets::Paragraph, Terminal};
use std::{fs, io, time::{Duration, Instant}};

const DOTS: [(u32,u32,u8);8] = [(0,0,0),(0,1,1),(0,2,2),(1,0,3),(1,1,4),(1,2,5),(0,3,6),(1,3,7)];
type Cell = Option<(char, Color)>;
type Grid = Vec<Vec<Cell>>;

fn luma(r:u8,g:u8,b:u8)->u8 { (0.299*r as f32+0.587*g as f32+0.114*b as f32) as u8 }
fn load_frames(dir:&str)->Vec<RgbaImage>{
    let mut f:Vec<_>=fs::read_dir(dir).unwrap_or_else(|_|panic!("no dir {dir}")).filter_map(|e|e.ok().map(|e|e.path()))
        .filter(|p|matches!(p.extension().and_then(|e|e.to_str()),Some("png")|Some("jpg"))).collect();
    f.sort();
    f.iter().map(|p|image::open(p).expect("frame").to_rgba8()).collect()
}
fn corner_key(img:&RgbaImage)->(u8,u8,u8){
    let (w,h)=(img.width(),img.height());
    let c=[img.get_pixel(0,0).0,img.get_pixel(w-1,0).0,img.get_pixel(0,h-1).0,img.get_pixel(w-1,h-1).0];
    let (mut r,mut g,mut b)=(0u32,0u32,0u32); for p in c {r+=p[0]as u32;g+=p[1]as u32;b+=p[2]as u32;}
    ((r/4)as u8,(g/4)as u8,(b/4)as u8)
}
fn grid(img:&RgbaImage, cols:u32, rows:u32, key:(u8,u8,u8))->Grid{
    let d=DynamicImage::ImageRgba8(img.clone()).resize_exact(cols*2,rows*4,imageops::FilterType::Lanczos3);
    let mut g=vec![vec![None;cols as usize];rows as usize];
    let tp=|p:[u8;4]|->bool{ if p[3]<128 {return true;} let (dr,dg,db)=(p[0]as i32-key.0 as i32,p[1]as i32-key.1 as i32,p[2]as i32-key.2 as i32); (dr*dr+dg*dg+db*db)<95*95 };
    for ty in 0..rows { for tx in 0..cols {
        let (px,py)=(tx*2,ty*4);
        let pix:Vec<[u8;4]>=DOTS.iter().map(|(dx,dy,_)|d.get_pixel(px+dx,py+dy).0).collect();
        let lum:Vec<Option<u8>>=pix.iter().map(|p| if tp(*p){None}else{Some(luma(p[0],p[1],p[2]))}).collect();
        let vis:Vec<u8>=lum.iter().filter_map(|l|*l).collect(); if vis.is_empty(){continue;}
        let avg=vis.iter().map(|&l|l as u32).sum::<u32>()/vis.len() as u32; let mut mask=0u32;
        for (i,(_,_,bit)) in DOTS.iter().enumerate(){ if let Some(l)=lum[i]{ if l as u32>=avg {mask|=1<<bit;}}}
        if mask==0 {continue;}
        let vp:Vec<&[u8;4]>=pix.iter().enumerate().filter(|(i,_)|lum[*i].is_some()).map(|(_,p)|p).collect();
        let n=vp.len()as u32; let (r,gg,b)=vp.iter().fold((0u32,0u32,0u32),|(a,b2,c),p|(a+p[0]as u32,b2+p[1]as u32,c+p[2]as u32));
        g[ty as usize][tx as usize]=Some((char::from_u32(0x2800+mask).unwrap_or(' '),Color::Rgb((r/n)as u8,(gg/n)as u8,(b/n)as u8)));
    }}
    g
}
fn stamp(buf:&mut Grid, g:&Grid, ax:i32, by:i32, sx:f32, sy:f32, flash:bool){
    let (gr,gc)=(g.len(),g[0].len());
    let (oh,ow)=(((gr as f32*sy)as i32).max(1),((gc as f32*sx)as i32).max(1));
    let (ox,oy)=(ax-ow/2, by-oh); let (rows,cols)=(buf.len()as i32, buf[0].len()as i32);
    for r in 0..oh { for c in 0..ow {
        let sr=((r as f32/sy)as usize).min(gr-1); let sc=((c as f32/sx)as usize).min(gc-1);
        let cell=g[sr][sc]; if cell.is_none(){continue;}
        let (x,y)=(ox+c, oy+r); if x<0||y<0||x>=cols||y>=rows{continue;}
        buf[y as usize][x as usize]= if flash {Some(('⣿',Color::Rgb(255,255,255)))} else {cell};
    }}
}
fn slash(buf:&mut Grid, cx:i32, cy:i32, p:f32){
    let (rows,cols)=(buf.len()as i32, buf[0].len()as i32);
    let reveal=(p*1.7).min(1.0); let fade=((1.0-p)*2.2).min(1.0); let len=20.0; let n=(len*reveal)as i32;
    for i in 0..n { let t=i as f32/len; let ax=cx as f32+9.0-18.0*t; let ay=cy as f32-8.0+16.0*t+3.0*(t*3.1416).sin();
        for (dx,dy) in [(0,0),(1,0),(0,1),(1,1)]{ let (x,y)=((ax as i32)+dx,(ay as i32)+dy);
            if x<0||y<0||x>=cols||y>=rows{continue;} let b=(230.0*fade)as u8;
            buf[y as usize][x as usize]=Some(('⣿',Color::Rgb(b,255,b))); } }
}
fn eo(u:f32)->f32{1.0-(1.0-u).powi(3)}

fn main()->io::Result<()>{
    let a:Vec<String>=std::env::args().collect();
    if a.len()<4 { eprintln!("usage: combat <atk_idle_dir> <atk_attack_dir> <target_idle_dir> [--fps N]"); std::process::exit(1); }
    let atk_idle=load_frames(&a[1]); let atk_atk=load_frames(&a[2]);
    let tgt_idle:Vec<RgbaImage>=load_frames(&a[3]).iter().map(|f|imageops::flip_horizontal(f)).collect();
    for (n,v) in [("atk_idle",&atk_idle),("atk_attack",&atk_atk),("target",&tgt_idle)]{ if v.is_empty(){eprintln!("no frames in {n}");std::process::exit(1);} }
    let mut fps=18u32; let mut i=4; while i<a.len(){ if a[i]=="--fps"&&i+1<a.len(){fps=a[i+1].parse().unwrap_or(18).max(1);i+=2;}else{i+=1;} }
    let (aik,aak,tk)=(corner_key(&atk_idle[0]),corner_key(&atk_atk[0]),corner_key(&tgt_idle[0]));

    enable_raw_mode()?; let mut so=io::stdout(); execute!(so,EnterAlternateScreen)?;
    let mut term=Terminal::new(CrosstermBackend::new(so))?;
    let size=term.size()?; let (cols,rows)=(size.width as u32, size.height.saturating_sub(1) as u32);
    let sh=(rows as f32*0.55) as u32;
    let swf=|img:&RgbaImage| ((img.width()as f32/img.height()as f32)*sh as f32) as u32;
    let ig:Vec<Grid>=atk_idle.iter().map(|f|grid(f,swf(f).max(2),sh,aik)).collect();
    let xg:Vec<Grid>=atk_atk.iter().map(|f|grid(f,swf(f).max(2),sh,aak)).collect();
    let tg:Vec<Grid>=tgt_idle.iter().map(|f|grid(f,swf(f).max(2),sh,tk)).collect();
    let by=(rows as i32)-1;
    let a_home=(cols as f32*0.22) as i32; let t_home=(cols as f32*0.66) as i32;
    let peak=((t_home-a_home) as f32*0.42) as f32;

    // state machine timeline: [idle .. attack .. recover] as fractions of the loop
    let total=72usize; let mut fr=0usize;
    let dur=Duration::from_millis((1000/fps)as u64); let mut last=Instant::now();
    loop {
        let t=fr as f32/total as f32;
        let ti=(fr + tg.len()/2) % tg.len();      // target idle, living

        // --- attacker: which clip + frame + transform ---
        let (agrid, mut adx, mut asx, mut asy);
        if t < 0.34 {                              // IDLE (bounce clip)
            agrid=&ig[fr % ig.len()]; adx=0; asx=1.0; asy=1.0;
        } else if t < 0.66 {                       // ATTACK (play lunge clip through)
            let u=(t-0.34)/0.32;
            let f=((u*(xg.len()-1) as f32).round() as usize).min(xg.len()-1);
            agrid=&xg[f];
            adx=(peak*(3.1416*u).sin()) as i32;    // approach & return
            asx=1.0+0.06*(3.1416*u).sin(); asy=1.0-0.04*(3.1416*u).sin();
        } else {                                   // RECOVER -> idle
            agrid=&ig[fr % ig.len()]; adx=0; asx=1.0; asy=1.0;
        }

        // --- impact window (attacker extended, ~mid-attack) ---
        let hit = t>=0.50 && t<0.60;
        let flash = t>=0.50 && t<0.55;
        let (mut tdx,mut tsx,mut tsy)=(0f32,1.0,1.0);
        if t>=0.50 && t<0.58 { let u=(t-0.50)/0.08; tdx=eo(u)*7.0; tsx=1.0+0.12*u; tsy=1.0-0.16*u; }
        else if t>=0.58 && t<0.78 { let u=(t-0.58)/0.20; tdx=7.0*(1.0-u); tsx=1.0+0.12*(1.0-u); tsy=1.0-0.16*(1.0-u); }

        let mut buf:Grid=vec![vec![None;cols as usize];rows as usize];
        stamp(&mut buf,&tg[ti], t_home+tdx as i32, by, tsx, tsy, flash);
        stamp(&mut buf, agrid, a_home+adx, by, asx, asy, false);
        if hit { slash(&mut buf, t_home, by-(sh as i32/2), (t-0.50)/0.10); }

        let lines:Vec<Line>=buf.iter().map(|row|{
            let mut spans=Vec::new(); let mut j=0;
            while j<row.len(){ match row[j]{
                None=>{let mut k=j; while k<row.len()&&row[k].is_none(){k+=1;} spans.push(Span::raw(" ".repeat(k-j))); j=k;}
                Some((_,col))=>{ let mut s=String::new(); while j<row.len(){ if let Some((ch,c))=row[j]{ if c==col{s.push(ch);j+=1;continue;} } break; } spans.push(Span::styled(s,Style::default().fg(col))); }
            }} Line::from(spans)
        }).collect();
        term.draw(|f| f.render_widget(Paragraph::new(lines), f.area()))?;
        if last.elapsed()>=dur { fr=(fr+1)%total; last=Instant::now(); }
        if event::poll(Duration::from_millis(4))?{ if let Event::Key(k)=event::read()?{ if matches!(k.code,KeyCode::Char('q')|KeyCode::Esc){break;} } }
    }
    disable_raw_mode()?; execute!(term.backend_mut(),LeaveAlternateScreen)?; Ok(())
}
