import subprocess, numpy as np, math, sys, json
F=sys.argv[1]; W,H=1920,1080; fps=30
p=subprocess.Popen(['ffmpeg','-v','error','-i',F,'-vf',f'fps={fps}','-f','rawvideo','-pix_fmt','rgb24','-'],stdout=subprocess.PIPE)
keys={'L':(1376,1030),'D':(1407,1030),'R':(1439,1030),'U':(1407,1000),'SP':(1190,1000)}
cx,cy=700,1158
out=[]; i=0
while True:
    b=p.stdout.read(W*H*3)
    if len(b)<W*H*3: break
    a=np.frombuffer(b,np.uint8).reshape(H,W,3)
    ks={k:int(a[y-4:y+5,x-6:x+7].astype(int).mean()>200) for k,(x,y) in keys.items()}
    reg=a[650:1045,250:1150].astype(int)
    m=(reg[:,:,0]>170)&(reg[:,:,1]>150)&(reg[:,:,2]<90)
    ys,xs=np.nonzero(m)
    ang=None
    if len(xs)>30:
        mx=xs.mean()+250; my=ys.mean()+650
        ang=math.degrees(math.atan2(mx-cx, cy-my))
    out.append((round(i/fps,3),ks,None if ang is None else round(ang,1)))
    i+=1
json.dump(out,open('/tmp/claude-0/wf3/track.json','w'))
print(i)
