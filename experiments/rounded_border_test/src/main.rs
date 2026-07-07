use engine_core::color::Rgba;
use engine_render::dots::{dots_to_grid, Dot, DotBuffer};
use engine_render::draw_grid;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;

const COLOR: Rgba = Rgba::rgb(0xff, 0xff, 0xff);

/// Draws a hollow rectangular border into `dots`, `thickness` dots wide per
/// edge, with a diagonal (Manhattan-distance) chamfer of `chamfer` dots
/// clipped off each of the 4 corners. `chamfer == 0` is a sharp square
/// corner.
fn draw_border(dots: &mut DotBuffer, thickness: usize, chamfer: usize, color: Rgba) {
    let cols = dots.cols();
    let rows = dots.rows();
    for row in 0..rows {
        for col in 0..cols {
            let d_left = col;
            let d_right = cols - 1 - col;
            let d_top = row;
            let d_bottom = rows - 1 - row;

            let in_border = d_left < thickness || d_right < thickness || d_top < thickness || d_bottom < thickness;
            if !in_border {
                continue;
            }

            // Chamfer clip: if within `chamfer` dots of TWO adjacent edges
            // (i.e. in a corner zone), only keep it if the Manhattan sum
            // clears the chamfer threshold (a 45-degree diagonal cut).
            let clipped = (d_left < chamfer && d_top < chamfer && d_left + d_top < chamfer)
                || (d_right < chamfer && d_top < chamfer && d_right + d_top < chamfer)
                || (d_left < chamfer && d_bottom < chamfer && d_left + d_bottom < chamfer)
                || (d_right < chamfer && d_bottom < chamfer && d_right + d_bottom < chamfer);
            if clipped {
                continue;
            }

            dots.set(col, row, Dot::Lit(color));
        }
    }
}

fn render_variant(label: &str, dot_cols: usize, dot_rows: usize, thickness: usize, chamfer: usize) {
    let mut dots = DotBuffer::new(dot_cols, dot_rows);
    draw_border(&mut dots, thickness, chamfer, COLOR);
    let grid = dots_to_grid(&dots);

    let cell_cols = (dot_cols as u16).div_ceil(2);
    let cell_rows = (dot_rows as u16).div_ceil(4);
    let mut terminal = Terminal::new(TestBackend::new(cell_cols, cell_rows)).unwrap();
    terminal
        .draw(|f| {
            let area = Rect::new(0, 0, cell_cols, cell_rows);
            draw_grid(f.buffer_mut(), area, &grid);
        })
        .unwrap();
    let buf = terminal.backend().buffer().clone();

    println!("--- {label} (thickness={thickness}, chamfer={chamfer}, {dot_cols}x{dot_rows} dots = {cell_cols}x{cell_rows} cells) ---");
    for y in 0..cell_rows {
        let mut line = String::new();
        for x in 0..cell_cols {
            line.push_str(buf.cell((x, y)).unwrap().symbol());
        }
        println!("{line}");
    }
    println!();
}

fn main() {
    // A representative small panel size: 20 dots wide x 16 dots tall (10x4 cells).
    let (w, h) = (20usize, 16usize);

    render_variant("thickness=1, chamfer=0 (sharp, thin)", w, h, 1, 0);
    render_variant("thickness=1, chamfer=1 (thin, tiny chamfer)", w, h, 1, 1);
    render_variant("thickness=2, chamfer=0 (thick, sharp)", w, h, 2, 0);
    render_variant("thickness=2, chamfer=1 (thick, small chamfer)", w, h, 2, 1);
    render_variant("thickness=2, chamfer=2 (thick, bigger chamfer)", w, h, 2, 2);
    render_variant("thickness=3, chamfer=2 (thicker, bigger chamfer)", w, h, 3, 2);
}
