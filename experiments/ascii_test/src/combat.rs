// combat — the "motion + effects" attack model, in braille.
// Attacker (left) does an anticipation → lunge; an AUTHORED braille slash-arc sweeps the target;
// the target flashes white and recoils. The creature never swings a coherent weapon — the hit is a
// drawn effect. General across any creature/action, zero diffusion gamble on the hard part.
//
// Usage: combat <attacker.png> <target.png> [--chroma auto|R,G,B] [--fps N]

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use image::{imageops, DynamicImage, GenericImageView, RgbaImage};
use ratatui::{
    backend::CrosstermBackend, style::{Color, Style}, text::{Line, Span}, widgets::Paragraph, Terminal,
};
use std::{io, time::{Duration, Instant}};

const DOTS: [(u32,u32,u8);8] = [(0,0,0),(0,1,1),(0,2,2),(1,0,3),(1,1,4),(1,2,5),(0,3,6),(1,3,7)];
type Cell = Option<(char, Color)>;

fn luma(r:u8,g:u8,b:u8)->u8 { (0.299*r as f32+0.587*g as f32+0.114*b as f32) as u8 }

fn corner_key(img:&RgbaImage)->(u8,u8,u8){
    let (w,h)=(img.width(),img.height());
    let c=[img.get_pixel(0,0).0,img.get_pixel(w-1,0).0,img.get_pixel(0,h-1).0,img.get_pixel(w-1,h-1).0];
    let (mut r,mut g,mut b)=(0u32,0u32,0u32);
    for p in c {r+=p[0] as u32;g+=p[1] as u32;b+=p[2] as u32;}
    ((r/4)as u8,(g/4)as u8,(b/4)as u8)
}

// Render an image to a grid of braille cells (transparent where alpha low or near key color).
fn grid(img:&RgbaImage, cols:u32, rows:u32, key:(u8,u8,u8))->Vec<Vec<Cell>>{
    let d=DynamicImage::ImageRgba8(img.clone()).resize_exact(cols*2,rows*4,imageops::FilterType::Lanczos3);
    let mut g=vec![vec![None;cols as usize];rows as usize];
    let tp=|p:[u8;4]|->bool{
        if p[3]<128 {return true;}
        let (dr,dg,db)=(p[0]as i32-key.0 as i32,p[1]as i32-key.1 as i32,p[2]as i32-key.2 as i32);
        (dr*dr+dg*dg+db*db) < 95*95
    };
    for ty in 0..rows { for tx in 0..cols {
        let (px,py)=(tx*2,ty*4);
        let pix:Vec<[u8;4]>=DOTS.iter().map(|(dx,dy,_)|d.get_pixel(px+dx,py+dy).0).collect();
        let lum:Vec<Option<u8>>=pix.iter().map(|p| if tp(*p){None}else{Some(luma(p[0],p[1],p[2]))}).collect();
        let vis:Vec<u8>=lum.iter().filter_map(|l|*l).collect();
        if vis.is_empty(){continue;}
        let avg=vis.iter().map(|&l|l as u32).sum::<u32>()/vis.len() as u32;
        let mut mask=0u32;
        for (i,(_,_,bit)) in DOTS.iter().enumerate(){ if let Some(l)=lum[i]{ if l as u32>=avg {mask|=1<<bit;}}}
        if mask==0 {continue;}
        let vp:Vec<&[u8;4]>=pix.iter().enumerate().filter(|(i,_)|lum[*i].is_some()).map(|(_,p)|p).collect();
        let n=vp.len() as u32;
        let (r,gg,b)=vp.iter().fold((0u32,0u32,0u32),|(a,b2,c),p|(a+p[0]as u32,b2+p[1]as u32,c+p[2]as u32));
        let ch=char::from_u32(0x2800+mask).unwrap_or(' ');
        g[ty as usize][tx as usize]=Some((ch,Color::Rgb((r/n)as u8,(gg/n)as u8,(b/n)as u8)));
    }}
    g
}

fn stamp(buf:&mut Vec<Vec<Cell>>, g:&Vec<Vec<Cell>>, ox:i32, oy:i32, flash:bool){
    let (rows,cols)=(buf.len() as i32, buf[0].len() as i32);
    for (r,row) in g.iter().enumerate(){ for (c,cell) in row.iter().enumerate(){
        if cell.is_none(){continue;}
        let (x,y)=(ox+c as i32, oy+r as i32);
        if x<0||y<0||x>=cols||y>=rows {continue;}
        buf[y as usize][x as usize]= if flash {Some(('⣿', Color::Rgb(255,255,255)))} else {*cell};
    }}
}

// Authored slash effect: a bright cyan arc sweeping down-right across (cx,cy). p in 0..1 reveals then fades.
fn slash(buf:&mut Vec<Vec<Cell>>, cx:i32, cy:i32, p:f32){
    let (rows,cols)=(buf.len() as i32, buf[0].len() as i32);
    let reveal = (p*1.6).min(1.0);              // leading edge sweeps in
    let fade   = ((1.0-p)*2.0).min(1.0);        // whole thing fades out at the end
    let len = 18.0;
    let n = (len*reveal) as i32;
    for i in 0..n {
        let t = i as f32/len;                    // 0..1 along the arc
        // arc: start upper-right, curve down-left through the target
        let ax = cx as f32 + 8.0 - 16.0*t;
        let ay = cy as f32 - 7.0 + 15.0*t + 3.0*(t*3.14).sin();
        for (dx,dy) in [(0,0),(1,0),(0,1)] {     // a little thickness
            let (x,y)=((ax as i32)+dx,(ay as i32)+dy);
            if x<0||y<0||x>=cols||y>=rows {continue;}
            let bright = (255.0*fade) as u8;
            buf[y as usize][x as usize]=Some(('⣿', Color::Rgb(bright, 255, bright)));
        }
    }
}

fn ease_out(u:f32)->f32{ 1.0-(1.0-u).powi(3) }

fn main()->io::Result<()>{
    let args:Vec<String>=std::env::args().collect();
    if args.len()<3 { eprintln!("usage: combat <attacker.png> <target.png> [--fps N]"); std::process::exit(1); }
    let atk=image::open(&args[1]).expect("attacker").to_rgba8();
    let tgt=image::open(&args[2]).expect("target");
    let tgt=imageops::flip_horizontal(&tgt.to_rgba8()); // face the attacker
    let mut fps=18u32;
    let mut i=3; while i<args.len(){ if args[i]=="--fps"&&i+1<args.len(){fps=args[i+1].parse().unwrap_or(18).max(1);i+=2;} else {i+=1;} }
    let atk_key=corner_key(&atk); let tgt_key=corner_key(&tgt);

    enable_raw_mode()?;
    let mut so=io::stdout(); execute!(so,EnterAlternateScreen)?;
    let mut term=Terminal::new(CrosstermBackend::new(so))?;
    let size=term.size()?;
    let (cols,rows)=(size.width as u32, size.height.saturating_sub(1) as u32);

    // sprites ~55% of screen height
    let sh=(rows as f32*0.55) as u32;
    let sw=|img:&RgbaImage,h:u32| ((img.width() as f32/img.height() as f32)*h as f32) as u32;
    let a_grid=grid(&atk, sw(&atk,sh), sh, atk_key);
    let t_grid=grid(&tgt, sw(&tgt,sh), sh, tgt_key);
    let base_y=(rows as i32)-(sh as i32)-1;
    let atk_home=(cols as f32*0.14) as i32;
    let tgt_home=(cols as f32*0.55) as i32;

    let total=48usize;
    let mut fr=0usize;
    let dur=Duration::from_millis((1000/fps) as u64);
    let mut last=Instant::now();
    loop {
        let t=fr as f32/total as f32;
        // attacker timeline: idle -> anticipate(pull back) -> lunge(fast in) -> recover
        let (mut adx, mut ady)=(0i32,0i32);
        if t>=0.15 && t<0.30 { let u=(t-0.15)/0.15; adx=-(6.0*u) as i32; ady=(2.0*u) as i32; }   // wind back
        else if t>=0.30 && t<0.45 { let u=ease_out((t-0.30)/0.15); adx=(-6.0+ (0.55*cols as f32-atk_home as f32-6.0)*u) as i32; } // lunge
        else if t>=0.45 && t<0.75 { let u=(t-0.45)/0.30; let peak=0.55*cols as f32-atk_home as f32-6.0; adx=(peak*(1.0-u)) as i32; } // return
        // target timeline: flash + recoil during impact
        let impact = t>=0.44 && t<0.54;
        let flash = t>=0.44 && t<0.50;
        let mut tdx=0i32;
        if t>=0.44 && t<0.52 { let u=(t-0.44)/0.08; tdx=(5.0*(1.0-(1.0-u).powi(2))) as i32; }
        else if t>=0.52 && t<0.70 { let u=(t-0.52)/0.18; tdx=(5.0*(1.0-u)) as i32; }
        // idle bob
        let bob=((t*6.28*2.0).sin()*1.0) as i32;

        let mut buf:Vec<Vec<Cell>>=vec![vec![None;cols as usize];rows as usize];
        stamp(&mut buf,&t_grid, tgt_home+tdx, base_y+bob, flash);
        stamp(&mut buf,&a_grid, atk_home+adx, base_y-bob, false);
        if impact { let p=(t-0.44)/0.10; slash(&mut buf, tgt_home+(a_grid[0].len() as i32/3), base_y+(sh as i32/2), p); }

        let lines:Vec<Line>=buf.iter().map(|row|{
            let mut spans=Vec::new(); let mut j=0;
            while j<row.len(){ match row[j]{
                None=>{let mut k=j; while k<row.len()&&row[k].is_none(){k+=1;} spans.push(Span::raw(" ".repeat(k-j))); j=k;}
                Some((_,col))=>{ let mut s=String::new();
                    while j<row.len(){ if let Some((ch,c))=row[j]{ if c==col {s.push(ch);j+=1;continue;} } break; }
                    spans.push(Span::styled(s,Style::default().fg(col))); }
            }}
            Line::from(spans)
        }).collect();
        term.draw(|f| f.render_widget(Paragraph::new(lines), f.area()))?;

        if last.elapsed()>=dur { fr=(fr+1)%total; last=Instant::now(); }
        if event::poll(Duration::from_millis(4))?{ if let Event::Key(k)=event::read()?{ if matches!(k.code,KeyCode::Char('q')|KeyCode::Esc){break;} } }
    }
    disable_raw_mode()?; execute!(term.backend_mut(),LeaveAlternateScreen)?; Ok(())
}
