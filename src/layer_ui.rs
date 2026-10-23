use super::*;
use crate::composition::{self,Transform};

#[derive(Default)]
pub(super) struct LayerView {
    pub live:bool,
    pub managed_edit:bool,
    pub canvas_focused:bool,
    pub textures:BTreeMap<String,egui::TextureHandle>,
    pub alpha:BTreeMap<String,image::GrayImage>,
    pub(super) selected:Option<String>,
    drag:Option<LayerDrag>,
    free_aspect:bool,
    pub(super) clipboard:Option<Vec<crate::composition::LayerClip>>,
}
struct LayerDrag {before:Project,transform:Transform,origin:Pos2,mode:u8}
fn points(t:&Transform,canvas:Rect)->[Pos2;4] {
    t.pixel_corners(canvas.width(),canvas.height()).map(|[x,y]|canvas.min+Vec2::new(x,y))
}
fn contains(t:&Transform,canvas:Rect,p:Pos2)->bool {
    let d=p-(canvas.min+Vec2::new(t.x*canvas.width(),t.y*canvas.height()));
    let (s,c)=(-t.rotation).to_radians().sin_cos();
    let (x,y)=(d.x*c-d.y*s,d.x*s+d.y*c);
    x.abs()<=t.width*canvas.width()/2. && y.abs()<=t.height*canvas.height()/2.
}
fn opaque(t:&Transform,canvas:Rect,p:Pos2,mask:Option<&image::GrayImage>)->bool {
    if !contains(t,canvas,p){return false;}
    let Some(mask)=mask else{return true;};
    let d=p-(canvas.min+Vec2::new(t.x*canvas.width(),t.y*canvas.height()));
    let (s,c)=(-t.rotation).to_radians().sin_cos();
    let u=(d.x*c-d.y*s)/(t.width*canvas.width())+0.5;
    let v=(d.x*s+d.y*c)/(t.height*canvas.height())+0.5;
    mask.get_pixel((u*mask.width() as f32).clamp(0.,(mask.width()-1) as f32) as u32,(v*mask.height() as f32).clamp(0.,(mask.height()-1) as f32) as u32)[0]>16
}
fn image_layer(painter:&egui::Painter,texture:&egui::TextureHandle,t:&Transform,canvas:Rect) {
    let mut mesh=egui::Mesh::with_texture(texture.id());
    let color=Color32::from_white_alpha((t.opacity*255.) as u8);
    for (pos,uv) in points(t,canvas).into_iter().zip([Pos2::new(0.,0.),Pos2::new(1.,0.),Pos2::new(1.,1.),Pos2::new(0.,1.)]) {
        mesh.vertices.push(egui::epaint::Vertex{pos,uv,color});
    }
    mesh.indices.extend([0,1,2,0,2,3]);painter.with_clip_rect(canvas).add(egui::Shape::mesh(mesh));
}
impl StudioApp {
    pub(super) fn independent_layer_edit(&mut self,command:&str)->bool {
        let Some(id)=self.layers.selected.clone() else{return false;};
        let Some(c)=self.project.composition_snapshot().clips.iter().find(|c|c.id==id&&!c.linked).cloned() else{return false;};
        if !c.editable(&self.project){self.message("Layer đang khóa",true);return true;}
        let before=self.project.clone();
        match self.project.edit_independent_layer(&id,command,self.timeline_view.cursor,&mut self.layers.clipboard){
            Ok(next)=>{self.layers.selected=next;if command!="copy"{self.timeline_view.history.record(before);self.transform_changed();self.request_frame();}},
            Err(e)=>self.message(format!("{e:#}"),true),
        }
        true
    }
    pub(super) fn select_timeline_layer(&mut self,track:usize) {
        self.project.ensure_composition();
        if let Some(s)=self.project.segments.get(self.selected) {
            self.layers.selected=self.project.composition.as_ref().unwrap().clips.iter()
                .find(|c|c.scene_id==s.id&&c.track==track).map(|c|c.id.clone());
        }
    }
    pub(super) fn transform_changed(&mut self) {
        self.layers.managed_edit=true;
        self.dirty=true;self.project.rendered=None;
        self.player.update_audio(&self.project);
        if self.preview_final {self.preview_final=false;self.preview_scene=None;self.texture=None;self.blurred_texture=None;self.request_frame();}
    }
    pub(super) fn set_layer_transform(&mut self,id:&str,t:Transform) {
        if self.project.apply_layer_transform(id,&t) {
            self.transform_changed();
        }
    }
    pub(super) fn layer_canvas(&mut self,ui:&mut egui::Ui,size:Vec2) {
        self.project.ensure_composition();
        let (outer,response)=ui.allocate_exact_size(size,Sense::click_and_drag());
        let painter=ui.painter_at(outer);painter.rect_filled(outer,8.,Color32::from_rgb(5,10,19));
        let (w,h)=self.project.output.dimensions();let ratio=w as f32/h as f32;
        let ch=(outer.height()-24.).min((outer.width()-24.)/ratio).max(2.);
        let canvas=Rect::from_center_size(outer.center(),Vec2::new(ch*ratio,ch));
        #[cfg(test)] self.hitboxes.insert("preview_canvas".into(),canvas);
        painter.rect_filled(canvas,0.,if self.project.output.editorial{Color32::from_rgb(244,240,230)}else{BG});
        let ms=self.timeline_view.cursor;
        let mut clips=self.project.composition.as_ref().unwrap().clips.clone();clips.sort_by_key(|c|c.z);
        for c in clips.iter().filter(|c|!c.audio()&&c.enabled(&self.project)&&c.active(ms)) {
            let t=composition::evaluated(&self.project,c,ms);
            let key=if c.motion.blurred(ms.saturating_sub(c.start_ms)+c.cue_in_ms){format!("{}#blur",c.id)}else{c.id.clone()};
            if let Some(texture)=self.layers.textures.get(&key).or_else(||self.layers.textures.get(&c.id)) {image_layer(&painter,texture,&t,canvas);}
        }
        let selected=self.layers.selected.clone();
        let mut mode=0;
        if let Some(c)=clips.iter().find(|c|Some(&c.id)==selected.as_ref()&&!c.audio()) {
            let t=composition::evaluated(&self.project,c,ms);let pts=points(&t,canvas);
            let locked=!c.editable(&self.project);let stroke=Stroke::new(1.5,if locked{MUTED}else{TEAL});
            for i in 0..4 {painter.line_segment([pts[i],pts[(i+1)%4]],stroke);}
            let mid=(pts[0].to_vec2()+pts[1].to_vec2())/2.;
            let center=canvas.min+Vec2::new(t.x*canvas.width(),t.y*canvas.height());
            let direction=(Pos2::new(mid.x,mid.y)-center).normalized();
            let rotate=Pos2::new(mid.x,mid.y)+direction*22.;
            painter.line_segment([Pos2::new(mid.x,mid.y),rotate],stroke);painter.circle_stroke(rotate,5.,stroke);
            for i in 0..8 {
                let point=if i%2==0 {pts[i/2]}else{pts[i/2]+(pts[(i/2+1)%4]-pts[i/2])*0.5};
                painter.rect_filled(Rect::from_center_size(point,Vec2::splat(7.)),1.,stroke.color);
                if response.interact_pointer_pos().is_some_and(|p|p.distance(point)<10.){mode=3+i as u8;}
            }
            if response.interact_pointer_pos().is_some_and(|p|p.distance(rotate)<12.){mode=2;}
            if locked {painter.text(center,Align2::CENTER_CENTER,"Đã khóa",FontId::proportional(12.),MUTED);}
            #[cfg(test)] self.hitboxes.insert("selected_layer".into(),Rect::from_two_pos(pts[0],pts[2]));
        }
        if response.clicked() || response.drag_started() {
            self.layers.canvas_focused=true;
            if let Some(pos)=response.interact_pointer_pos() {
                let hits:Vec<_>=clips.iter().rev().filter(|c|!c.audio()&&c.enabled(&self.project)&&c.active(ms)&&self.layers.textures.contains_key(&c.id)&&opaque(&composition::evaluated(&self.project,c,ms),canvas,pos,self.layers.alpha.get(&c.id))).collect();
                if mode==0 {
                    let alt=ui.input(|i|i.modifiers.alt);
                    let index=if alt {hits.iter().position(|c|Some(&c.id)==selected.as_ref()).map(|n|(n+1)%hits.len()).unwrap_or(0)}else{0};
                    if let Some(c)=hits.get(index) {self.layers.selected=Some(c.id.clone());self.inspector=0;}
                }
                if response.drag_started() && !self.busy {
                    if let Some(c)=clips.iter().find(|c|Some(&c.id)==self.layers.selected.as_ref()) {
                        if c.editable(&self.project){self.layers.drag=Some(LayerDrag{before:self.project.clone(),transform:c.transform.clone(),origin:ui.input(|i|i.pointer.press_origin()).unwrap_or(pos),mode});}
                    }
                }
            }
        }
        if response.dragged() {
            if let (Some(drag),Some(id),Some(pos))=(&self.layers.drag,self.layers.selected.clone(),response.interact_pointer_pos()) {
                let mut t=drag.transform.clone();let d=pos-drag.origin;
                match drag.mode {
                    3..=10 if self.layers.free_aspect=>{
                        let center=canvas.min+Vec2::new(t.x*canvas.width(),t.y*canvas.height());let d=pos-center;
                        let (s,c)=(-t.rotation).to_radians().sin_cos();let (x,y)=(d.x*c-d.y*s,d.x*s+d.y*c);let handle=drag.mode-3;
                        if handle%2==0 || handle==3 || handle==7 {t.width=(x.abs()*2./canvas.width()).clamp(0.01,10.);}
                        if handle%2==0 || handle==1 || handle==5 {t.height=(y.abs()*2./canvas.height()).clamp(0.01,10.);}
                    },
                    3..=10=>{let center=canvas.min+Vec2::new(t.x*canvas.width(),t.y*canvas.height());
                        let old=drag.origin.distance(center).max(1.);let factor=(pos.distance(center)/old).clamp(0.05,10.);
                        t.width=(t.width*factor).clamp(0.01,10.);t.height=(t.height*factor).clamp(0.01,10.);},
                    2=>{let center=canvas.min+Vec2::new(t.x*canvas.width(),t.y*canvas.height());let a=drag.origin-center;let b=pos-center;
                        t.rotation+=(b.y.atan2(b.x)-a.y.atan2(a.x)).to_degrees();},
                    _=>{t.x+=d.x/canvas.width();t.y+=d.y/canvas.height();
                        if self.project.timeline.snapping && !ui.input(|i|i.modifiers.alt) {
                            for (v,extent,size) in [(&mut t.x,canvas.width(),t.width),(&mut t.y,canvas.height(),t.height)] {
                                for target in [0.5,size/2.,1.-size/2.] {if (*v-target).abs()*extent<5. {*v=target;break;}}
                            }
                        }
                    },
                }
                self.set_layer_transform(&id,t);
            }
        }
        if response.drag_stopped(){if let Some(drag)=self.layers.drag.take(){self.layers.managed_edit=true;self.timeline_view.history.record(drag.before);}}
        painter.rect_stroke(canvas,0.,Stroke::new(1.,EDGE),egui::StrokeKind::Inside);
        if !ui.ctx().wants_keyboard_input() && self.layers.canvas_focused && !self.busy {
            if let Some(id)=self.layers.selected.clone() {
                if let Some(c)=clips.iter().find(|c|c.id==id&&c.editable(&self.project)) {
                    let step=if ui.input(|i|i.modifiers.shift){10.}else{1.};let mut d=Vec2::ZERO;
                    ui.input(|i|{if i.key_pressed(egui::Key::ArrowLeft){d.x-=step;}
 if i.key_pressed(egui::Key::ArrowRight){d.x+=step;}
 if i.key_pressed(egui::Key::ArrowUp){d.y-=step;}
 if i.key_pressed(egui::Key::ArrowDown){d.y+=step;}});
                    if d!=Vec2::ZERO {let before=self.project.clone();let mut t=c.transform.clone();t.x+=d.x/w as f32;t.y+=d.y/h as f32;self.set_layer_transform(&id,t);self.timeline_view.history.record(before);}
                }
            }
        }
    }
    pub(super) fn layer_inspector(&mut self,ui:&mut egui::Ui) {
        self.project.ensure_composition();
        ui.spacing_mut().interact_size.y=24.;
        ui.spacing_mut().button_padding=Vec2::new(6.,3.);
        let comp=self.project.composition.as_ref().unwrap();
        let scene=self.project.segments.get(self.selected).map(|s|s.id.as_str()).unwrap_or("");
        let clips:Vec<_>=comp.clips.iter().filter(|c|c.scene_id==scene||c.active(self.timeline_view.cursor)).cloned().collect();

        ui.horizontal(|ui|{
            ui.label(RichText::new("LAYER").strong().color(MUTED));
            egui::ComboBox::from_id_salt("selected_layer").selected_text(self.layers.selected.as_ref().and_then(|id|clips.iter().find(|c|&c.id==id)).map(|c|composition::label(&self.project,c)).unwrap_or_else(||"Chọn layer…".into()))
                .show_ui(ui,|ui|{for c in &clips{ui.selectable_value(&mut self.layers.selected,Some(c.id.clone()),composition::label(&self.project,c));}});
        });
        let Some(id)=self.layers.selected.clone() else {ui.label(RichText::new("Chọn layer trên canvas hoặc timeline.").color(MUTED));ui.separator();return;};
        let Some(c)=self.project.composition.as_ref().unwrap().clips.iter().find(|c|c.id==id).cloned() else{return;};
        let before=self.project.clone();let mut t=c.transform.clone();let (w,h)=self.project.output.dimensions();let (mut x,mut y)=t.pixels(w,h);
        ui.add_enabled_ui(c.editable(&self.project),|ui|{
            let sync=ui.checkbox(&mut self.project.timeline.tracks[c.track].sync_parameters,"Đồng bộ thông số với các layer cùng loại")
                .on_hover_text("Bật để lấy layer đang chọn làm mẫu trong toàn dự án. Đồng bộ hình học / thứ tự, hoặc âm lượng / fade. Bỏ qua layer, cảnh và nhóm đang khóa.");
            #[cfg(test)] self.hitboxes.insert("layer_sync_parameters".into(),sync.rect);
            if sync.changed() {
                if self.project.sync_layer_parameters(&id){self.transform_changed();}
                self.dirty=true;self.layers.managed_edit=true;
                self.timeline_view.history.record(before.clone());
            }
            if self.project.timeline.tracks[c.track].sync_parameters {
                let count=self.project.layer_parameter_targets(&id,false).len();
                ui.label(RichText::new(format!("Đồng bộ {count} layer {} có thể chỉnh sửa",composition::name(c.track))).size(12.).color(MUTED));
            }
            if !c.audio() {

                let position_changed=ui.horizontal(|ui|{
                    ui.label("X");let rx=ui.add(egui::DragValue::new(&mut x).speed(1.).range(-10000.0..=10000.0).suffix(" px"));
                    ui.label("Y");let ry=ui.add(egui::DragValue::new(&mut y).speed(1.).range(-10000.0..=10000.0).suffix(" px"));
                    #[cfg(test)] {self.hitboxes.insert("layer_x".into(),rx.rect);self.hitboxes.insert("layer_y".into(),ry.rect);self.hitboxes.insert("layer_inspector_clip".into(),ui.clip_rect());}
                    rx.changed()||ry.changed()
                }).inner;
                if position_changed {t.set_pixels(x,y,w,h);}
                ui.checkbox(&mut self.layers.free_aspect,"Co giãn tự do (bỏ khóa tỉ lệ)");
                let old_width=t.width;
                ui.add(egui::Slider::new(&mut t.width,0.01..=3.).text("Rộng / khung"));
                if (old_width-t.width).abs()>f32::EPSILON{t.height*=t.width/old_width;}
                ui.add(egui::DragValue::new(&mut t.rotation).speed(0.5).suffix("°"));
                ui.add(egui::Slider::new(&mut t.opacity,0.0..=1.).text("Độ hiện"));
                ui.horizontal_wrapped(|ui|{
                    if ui.button("Căn giữa").clicked(){t.x=0.5;t.y=0.5;}
                    if ui.button("Đưa về khung").clicked(){t.x=t.x.clamp(t.width.min(1.)/2.,1.-t.width.min(1.)/2.);t.y=t.y.clamp(t.height.min(1.)/2.,1.-t.height.min(1.)/2.);}
                    if ui.button("Reset").clicked(){if let Some(base)=composition::Composition::from_project(&self.project).clips.iter().find(|v|v.id==id){t=base.transform.clone();}}
                    if c.track==crate::timeline::AVATAR && ui.button("Đúng tỉ lệ gốc").on_hover_text("Fit nhân vật trong vùng hiện tại, giữ vị trí và góc xoay").clicked(){t.contain_aspect(composition::avatar_aspect(c.media.as_deref()),w,h);}
                });
            }
            let mut linked=c.linked;let mut gain=c.gain;let mut z=c.z;let mut fi=c.fade_in_ms;let mut fo=c.fade_out_ms;
            ui.checkbox(&mut linked,"Liên kết timing với cảnh");
            if c.evidence_id.is_some(){
                let label=if c.group.is_some(){"Bỏ nhóm ảnh / chú thích"}else{"Nhóm ảnh / chú thích"};
                if ui.button(label).clicked(){
                    let group=if c.group.is_some(){None}else{Some(format!("{}:ev:{}",c.scene_id,c.evidence_id.as_ref().unwrap()))};
                    let targets:Vec<_>=self.project.composition.as_ref().unwrap().clips.iter().filter(|v|v.scene_id==c.scene_id&&v.evidence_id==c.evidence_id&&v.id.contains(":ev:")).cloned().collect();
                    if targets.iter().all(|v|v.editable(&self.project)){
                        for v in &mut self.project.composition.as_mut().unwrap().clips{if targets.iter().any(|t|t.id==v.id){v.group=group.clone();}}
                        self.timeline_view.history.record(before.clone());self.transform_changed();
                    }
                }
            }
            if c.audio(){
                ui.add(egui::Slider::new(&mut gain,0.0..=2.).text("Âm lượng"));
                if c.background_original(&self.project) {
                    ui.label(format!("Mức thực tế: {:.0}% (nhân với âm lượng nền của dự án)",c.effective_gain(&self.project)*100.));
                }
                ui.horizontal(|ui|{ui.label("Fade vào/ra");ui.add(egui::DragValue::new(&mut fi).range(0..=c.duration_ms).suffix(" ms"));ui.add(egui::DragValue::new(&mut fo).range(0..=c.duration_ms).suffix(" ms"));});
            }
            else{ui.add(egui::DragValue::new(&mut z).prefix("Thứ tự "));}
            if linked!=c.linked||gain!=c.gain||z!=c.z||fi!=c.fade_in_ms||fo!=c.fade_out_ms {
                let clip=self.project.composition.as_mut().unwrap().clips.iter_mut().find(|c|c.id==id).unwrap();clip.linked=linked;clip.gain=gain;clip.z=z;clip.fade_in_ms=fi;clip.fade_out_ms=fo;
                self.project.sync_layer_parameters(&id);
                self.timeline_view.history.record(before.clone());self.transform_changed();
            }
            egui::CollapsingHeader::new("Timing riêng layer").show(ui,|ui|{
                let mut start=c.start_ms;let mut trim=0u64;let mut duration=c.duration_ms;
                ui.label("Vị trí timeline / bỏ đầu media / độ dài");
                let a=ui.add(egui::DragValue::new(&mut start).speed(10).range(0..=7_200_000).suffix(" ms"));
                let b=ui.add(egui::DragValue::new(&mut trim).speed(10).range(0..=c.duration_ms.saturating_sub(100)).suffix(" ms"));
                let d=ui.add(egui::DragValue::new(&mut duration).speed(10).range(100..=c.duration_ms).suffix(" ms"));
                if a.changed()||b.changed()||d.changed(){
                    let result=self.project.time_layer(&id,start,trim,duration.min(c.duration_ms.saturating_sub(trim)));
                    match result {Ok(())=>{self.timeline_view.history.record(before.clone());self.transform_changed();self.request_frame();},Err(e)=>self.message(format!("{e:#}"),true)}
                }
            });
        });
        if t!=c.transform {
            self.set_layer_transform(&id,t);self.timeline_view.history.record(before);
        }
        egui::CollapsingHeader::new("Danh sách layer • ẩn / khóa").id_salt("layer_visibility").show(ui,|ui|{
        ui.spacing_mut().interact_size.y=22.;
        ui.spacing_mut().button_padding=Vec2::new(4.,2.);
        for clip in &clips {
            ui.horizontal(|ui|{
                let selected=self.layers.selected.as_ref()==Some(&clip.id);
                let r=ui.selectable_label(selected,composition::label(&self.project,clip));
                #[cfg(test)] self.hitboxes.insert(format!("layer_{}",clip.track),r.rect);
                if r.clicked(){self.layers.selected=Some(clip.id.clone());}
                let mut visible=clip.visible;let mut locked=clip.locked;
                ui.checkbox(&mut visible,"Hiện");ui.checkbox(&mut locked,"Khóa");
                if visible!=clip.visible||locked!=clip.locked {
                    let before=self.project.clone();let c=self.project.composition.as_mut().unwrap().clips.iter_mut().find(|c|c.id==clip.id).unwrap();
                    c.visible=visible;c.locked=locked;self.timeline_view.history.record(before);self.transform_changed();if !clip.audio(){self.request_frame();}
                }
            });
        }
        });
        ui.separator();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sync_checkbox_is_per_track_applies_immediately_and_is_one_undo() {
        use crate::timeline;
        for width in [240.,308.,560.] {
            let temp=tempfile::tempdir().unwrap();let ctx=egui::Context::default();let mut app=StudioApp::from_context(&ctx);app.player.stop();app.capture_path=None;
            app.project=Project{root:temp.path().into(),sources:vec![Source{id:"s".into(),width:640,height:360,..Default::default()}],segments:(0..3).map(|i|Segment{id:format!("s{i}"),kind:"review".into(),source_id:"s".into(),duration_ms:1000,out_ms:1000,locked:i==2,..Default::default()}).collect(),..Default::default()};
            app.project.ensure_composition();app.layers.selected=Some("s0-1".into());
            let comp=app.project.composition.as_mut().unwrap();comp.clips.iter_mut().find(|c|c.id=="s0-1").unwrap().transform.x=0.2;
            let first=comp.clips.iter().find(|c|c.id=="s0-1").unwrap().transform.clone();let old=comp.clips.iter().find(|c|c.id=="s1-1").unwrap().transform.clone();
            let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(width+24.,1200.));
            let draw=|app:&mut StudioApp,events|{let _=ctx.run(egui::RawInput{screen_rect:Some(screen),events,..Default::default()},|ctx|{egui::CentralPanel::default().show(ctx,|ui|app.layer_inspector(ui));});};
            draw(&mut app,vec![]);draw(&mut app,vec![]);app.timeline_view.history=Default::default();
            let rect=app.hitboxes["layer_sync_parameters"];assert!(screen.contains_rect(rect),"Sync checkbox outside {width}px: {rect:?}");assert!(!rect.intersects(app.hitboxes["layer_x"]));
            let click=|pos|vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:true,modifiers:egui::Modifiers::NONE},egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:false,modifiers:egui::Modifiers::NONE}];
            draw(&mut app,click(rect.center()));
            assert!(app.project.timeline.tracks[timeline::AVATAR].sync_parameters&&app.dirty);assert!(!app.project.timeline.tracks[timeline::VIDEO].sync_parameters);
            let comp=app.project.composition.as_ref().unwrap();assert_eq!(comp.clips.iter().find(|c|c.id=="s1-1").unwrap().transform,first);assert_eq!(comp.clips.iter().find(|c|c.id=="s2-1").unwrap().transform,old);
            assert_eq!(app.timeline_view.history.past.len(),1);assert!(app.timeline_view.history.undo(&mut app.project));
            assert!(!app.project.timeline.tracks[timeline::AVATAR].sync_parameters);assert_eq!(app.project.composition.as_ref().unwrap().clips.iter().find(|c|c.id=="s1-1").unwrap().transform,old);
            assert!(app.timeline_view.history.redo(&mut app.project));assert!(app.project.timeline.tracks[timeline::AVATAR].sync_parameters);
            app.layers.selected=Some("s0-0".into());draw(&mut app,vec![]);assert!(!app.project.timeline.tracks[timeline::VIDEO].sync_parameters);
            app.layers.selected=Some("s0-1".into());draw(&mut app,vec![]);let pos=app.hitboxes["layer_sync_parameters"].center();draw(&mut app,click(pos));
            assert!(!app.project.timeline.tracks[timeline::AVATAR].sync_parameters);
            let mut moved=first;moved.x=0.7;app.set_layer_transform("s0-1",moved);
            assert_eq!(app.project.composition.as_ref().unwrap().clips.iter().find(|c|c.id=="s1-1").unwrap().transform.x,0.2);
        }
    }
    #[test]
    fn all_evidence_transforms_keep_annotations_attached_and_skip_locked_groups(){
        use crate::evidence::{EvidenceItem,Annotation};use crate::timeline;
        let temp=tempfile::tempdir().unwrap();let ctx=egui::Context::default();let mut app=StudioApp::from_context(&ctx);app.player.stop();app.capture_path=None;
        let item=EvidenceItem{source_id:"s".into(),asset:Some("frame.png".into()),annotation_asset:Some("ann.png".into()),annotations:vec![Annotation::default()],..Default::default()};
        app.project=Project{root:temp.path().into(),sources:vec![Source{id:"s".into(),..Default::default()}],segments:(0..3).map(|i|Segment{id:format!("scene-{i}"),kind:"host".into(),source_id:"s".into(),duration_ms:4000,evidence_items:vec![item.clone()],locked:i==2,..Default::default()}).collect(),..Default::default()};
        app.project.ensure_composition();let comp=app.project.composition.as_ref().unwrap();let id=comp.clips.iter().find(|c|c.track==timeline::EVIDENCE).unwrap().id.clone();
        let locked=comp.clips.iter().find(|c|c.scene_id=="scene-2"&&c.track==timeline::ANNOTATION).unwrap().transform.clone();
        app.project.timeline.tracks[timeline::EVIDENCE].sync_parameters=true;let t=Transform{x:0.1,y:0.2,width:0.2,height:0.2,..Default::default()};app.set_layer_transform(&id,t.clone());
        for c in app.project.composition.as_ref().unwrap().clips.iter().filter(|c|c.evidence_id.is_some()&&!c.audio()){
            assert_eq!(c.transform,if c.scene_id=="scene-2"{locked.clone()}else{t.clone()});
        }
    }
    #[test]
    fn applying_transforms_to_all_clips_preserves_locked_scenes() {
        let temp=tempfile::tempdir().unwrap();let ctx=egui::Context::default();
        let mut app=StudioApp::from_context(&ctx);app.capture_path=None;
        app.project=Project{root:temp.path().into(),sources:vec![Source{id:"s".into(),width:640,height:360,..Default::default()}],
            segments:vec![Segment{id:"a".into(),source_id:"s".into(),duration_ms:1000,..Default::default()},
                Segment{id:"b".into(),source_id:"s".into(),duration_ms:1000,locked:true,..Default::default()}],..Default::default()};
        app.project.ensure_composition();app.project.timeline.tracks[crate::timeline::VIDEO].sync_parameters=true;
        let clips=&app.project.composition.as_ref().unwrap().clips;
        let first=clips.iter().find(|c|c.scene_id=="a"&&c.track==crate::timeline::VIDEO).unwrap().clone();
        let second=clips.iter().find(|c|c.scene_id=="b"&&c.track==crate::timeline::VIDEO).unwrap().clone();
        let mut changed=first.transform.clone();changed.x=2.;
        app.set_layer_transform(&first.id,changed.clone());
        let clips=&app.project.composition.as_ref().unwrap().clips;
        assert_eq!(clips.iter().find(|c|c.id==first.id).unwrap().transform,changed);
        assert_eq!(clips.iter().find(|c|c.id==second.id).unwrap().transform,second.transform);
    }
}
