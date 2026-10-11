program main
implicit none
integer,target::a(2,3),b(3,1),cube(2,2,2),high(2,1,1,1,1,1,1,1,1,1,1,1,1,1,1)
integer,pointer::p(:,:),q(:,:),r(:,:,:),s(:,:,:,:,:,:,:,:,:,:,:,:,:,:,:)
a=reshape([1,2,3,4,5,6],[2,3])
b=7
p=>a
q=>p
p=>b
q(2,3)=9
print *,shape(p),shape(q),a(2,3),sum(q)
cube=2
r=>cube
r(2,1,2)=10
print *,shape(r),sum(r)
high=4
s=>high
s(2,1,1,1,1,1,1,1,1,1,1,1,1,1,1)=8
print *,size(s),sum(s),high(2,1,1,1,1,1,1,1,1,1,1,1,1,1,1)
p=>a(:,3:1:-1)
print *,p
end program main
